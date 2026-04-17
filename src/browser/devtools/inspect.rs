use chromiumoxide::cdp::browser_protocol::{
    dom::{
        BackendNodeId, BoxModel, DescribeNodeParams, GetBoxModelParams, GetDocumentParams,
        GetOuterHtmlParams, GetSearchResultsParams, Node, NodeId, PerformSearchParams,
        QuerySelectorParams, Rgba,
    },
    overlay::{
        EventInspectModeCanceled, EventInspectNodeRequested, HideHighlightParams, HighlightConfig,
        HighlightNodeParams, InspectMode, SetInspectModeParams,
    },
};
use tokio::time::timeout;

use super::super::{
    actor::BrowserActor,
    types::{BoundingBoxInfo, BrowserValue, InspectNodeInfo, Locator},
    util::{flat_attributes_to_map, locator_name, serialize_to_browser_value},
    *,
};

const INSPECT_PICK_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
struct ResolvedNode {
    node_id: NodeId,
    backend_node_id: BackendNodeId,
}

impl BrowserActor {
    pub(in crate::browser) async fn inspect_locator_info(
        &mut self,
        locator: Locator,
    ) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        let info = self.inspect_page_locator(&page, &locator).await?;
        serialize_to_browser_value(&info, "failed to serialize inspected node info")
    }

    pub(in crate::browser) async fn inspect_highlight_locator(
        &mut self,
        locator: Locator,
    ) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        let resolved = self.resolve_locator_node(&page, &locator).await?;
        page.execute(
            HighlightNodeParams::builder()
                .highlight_config(default_highlight_config())
                .node_id(resolved.node_id.clone())
                .build()
                .expect("highlight params should build"),
        )
        .await
        .with_context(|| {
            format!(
                "failed to highlight {} `{}`",
                locator_name(locator.kind()),
                locator.raw()
            )
        })?;

        let info = self
            .inspect_backend_node(&page, resolved.backend_node_id)
            .await?;
        serialize_to_browser_value(&info, "failed to serialize highlighted node info")
    }

    pub(in crate::browser) async fn inspect_hide_highlight(&mut self) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        self.disable_inspect_mode(&page).await?;
        Ok(BrowserValue::Boolean(true))
    }

    pub(in crate::browser) async fn inspect_pick(
        &mut self,
        timeout_ms: u64,
    ) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        let mut pick_events = page
            .event_listener::<EventInspectNodeRequested>()
            .await
            .context("failed to subscribe to inspect-node events")?;
        let mut cancel_events = page
            .event_listener::<EventInspectModeCanceled>()
            .await
            .context("failed to subscribe to inspect-mode cancel events")?;

        page.execute(
            SetInspectModeParams::builder()
                .mode(InspectMode::SearchForNode)
                .highlight_config(default_highlight_config())
                .build()
                .expect("inspect mode params should build"),
        )
        .await
        .context("failed to enable inspect mode")?;

        let timeout_duration = if timeout_ms == 0 {
            INSPECT_PICK_TIMEOUT
        } else {
            Duration::from_millis(timeout_ms)
        };
        let picked_backend_node = timeout(timeout_duration, async {
            loop {
                tokio::select! {
                    event = pick_events.next() => {
                        match event {
                            Some(event) => {
                                return Ok(event.as_ref().clone().backend_node_id);
                            }
                            None => {
                                return Err(anyhow!("inspect mode closed before a node was selected"));
                            }
                        }
                    }
                    event = cancel_events.next() => {
                        match event {
                            Some(_) => {
                                return Err(anyhow!("inspect mode was canceled before a node was selected"));
                            }
                            None => {
                                return Err(anyhow!("inspect mode closed before a node was selected"));
                            }
                        }
                    }
                }
            }
        })
        .await;

        let _ = self.disable_inspect_mode(&page).await;

        let backend_node_id = match picked_backend_node {
            Ok(result) => result?,
            Err(_) => bail!("timed out waiting for a picked node"),
        };
        let info = self.inspect_backend_node(&page, backend_node_id).await?;

        serialize_to_browser_value(&info, "failed to serialize picked node info")
    }

    async fn inspect_page_locator(
        &self,
        page: &Page,
        locator: &Locator,
    ) -> Result<InspectNodeInfo> {
        let resolved = self.resolve_locator_node(page, locator).await?;
        self.inspect_backend_node(page, resolved.backend_node_id)
            .await
    }

    async fn resolve_locator_node(&self, page: &Page, locator: &Locator) -> Result<ResolvedNode> {
        match locator {
            Locator::Css(selector) => {
                let document = page
                    .execute(GetDocumentParams::builder().depth(0).build())
                    .await
                    .context("failed to get DOM root document")?;
                let query = page
                    .execute(QuerySelectorParams::new(
                        document.result.root.node_id,
                        selector.clone(),
                    ))
                    .await
                    .with_context(|| format!("failed to query selector `{selector}` via CDP"))?;
                if *query.result.node_id.inner() == 0 {
                    bail!("selector `{selector}` did not match any node");
                }
                let described = page
                    .execute(
                        DescribeNodeParams::builder()
                            .node_id(query.result.node_id.clone())
                            .build(),
                    )
                    .await
                    .with_context(|| format!("failed to describe selector `{selector}`"))?;
                Ok(ResolvedNode {
                    node_id: query.result.node_id,
                    backend_node_id: described.result.node.backend_node_id,
                })
            }
            Locator::XPath(xpath) => {
                let search = page
                    .execute(PerformSearchParams::new(xpath.clone()))
                    .await
                    .with_context(|| format!("failed to search xpath `{xpath}` via CDP"))?;
                if search.result.result_count <= 0 {
                    bail!("xpath `{xpath}` did not match any node");
                }
                let results = page
                    .execute(GetSearchResultsParams::new(search.result.search_id, 0, 1))
                    .await
                    .with_context(|| format!("failed to resolve xpath `{xpath}` search results"))?;
                let node_id = results
                    .result
                    .node_ids
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow!("xpath `{xpath}` did not return a node id"))?;
                let described = page
                    .execute(
                        DescribeNodeParams::builder()
                            .node_id(node_id.clone())
                            .build(),
                    )
                    .await
                    .with_context(|| format!("failed to describe xpath `{xpath}`"))?;
                Ok(ResolvedNode {
                    node_id,
                    backend_node_id: described.result.node.backend_node_id,
                })
            }
        }
    }

    async fn inspect_backend_node(
        &self,
        page: &Page,
        backend_node_id: BackendNodeId,
    ) -> Result<InspectNodeInfo> {
        let page_id = page.target_id().as_ref().to_string();
        let described = page
            .execute(
                DescribeNodeParams::builder()
                    .backend_node_id(backend_node_id)
                    .build(),
            )
            .await
            .with_context(|| {
                format!(
                    "failed to describe backend node `{}`",
                    backend_node_id.inner()
                )
            })?;
        let node = described.result.node;
        let outer_html = page
            .execute(
                GetOuterHtmlParams::builder()
                    .backend_node_id(backend_node_id)
                    .include_shadow_dom(true)
                    .build(),
            )
            .await
            .with_context(|| {
                format!(
                    "failed to read outer html for backend node `{}`",
                    backend_node_id.inner()
                )
            })?;
        let bounding_box = match page
            .execute(
                GetBoxModelParams::builder()
                    .backend_node_id(backend_node_id)
                    .build(),
            )
            .await
        {
            Ok(model) => box_model_to_bounding_box(&model.result.model),
            Err(_) => None,
        };

        Ok(node_to_inspect_info(
            page_id,
            node,
            outer_html.result.outer_html,
            bounding_box,
        ))
    }

    async fn disable_inspect_mode(&self, page: &Page) -> Result<()> {
        page.execute(
            SetInspectModeParams::builder()
                .mode(InspectMode::None)
                .build()
                .expect("inspect mode reset params should build"),
        )
        .await
        .context("failed to disable inspect mode")?;
        page.execute(HideHighlightParams::default())
            .await
            .context("failed to hide inspect highlight")?;
        Ok(())
    }
}

fn default_highlight_config() -> HighlightConfig {
    HighlightConfig::builder()
        .show_info(true)
        .show_styles(true)
        .show_accessibility_info(true)
        .show_extension_lines(true)
        .content_color(rgba(111, 168, 220, 0.35))
        .padding_color(rgba(147, 196, 125, 0.25))
        .border_color(rgba(255, 229, 153, 0.8))
        .margin_color(rgba(246, 178, 107, 0.35))
        .build()
}

fn rgba(r: i64, g: i64, b: i64, a: f64) -> Rgba {
    Rgba::builder()
        .r(r)
        .g(g)
        .b(b)
        .a(a)
        .build()
        .expect("rgba should build")
}

fn node_to_inspect_info(
    page_id: String,
    node: Node,
    outer_html: String,
    bounding_box: Option<BoundingBoxInfo>,
) -> InspectNodeInfo {
    InspectNodeInfo {
        page_id,
        node_id: *node.node_id.inner(),
        backend_node_id: *node.backend_node_id.inner(),
        node_type: node.node_type,
        node_name: node.node_name,
        local_name: node.local_name,
        node_value: node.node_value,
        child_node_count: node.child_node_count,
        attributes: node
            .attributes
            .map(flat_attributes_to_map)
            .unwrap_or_default(),
        frame_id: node.frame_id.map(|frame_id| frame_id.as_ref().to_string()),
        is_svg: node.is_svg,
        is_scrollable: node.is_scrollable,
        outer_html,
        bounding_box,
    }
}

fn box_model_to_bounding_box(model: &BoxModel) -> Option<BoundingBoxInfo> {
    let border = model.border.inner();
    if border.len() < 8 {
        return None;
    }

    let xs = [border[0], border[2], border[4], border[6]];
    let ys = [border[1], border[3], border[5], border[7]];
    let min_x = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let max_x = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let min_y = ys.iter().copied().fold(f64::INFINITY, f64::min);
    let max_y = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    Some(BoundingBoxInfo {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    })
}

#[cfg(test)]
mod tests {
    use chromiumoxide::cdp::browser_protocol::dom::{BackendNodeId, BoxModel, Node, NodeId, Quad};

    use super::*;

    #[test]
    fn box_model_to_bounding_box_maps_border_quad() {
        let model = BoxModel::builder()
            .content(Quad::new(vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0]))
            .padding(Quad::new(vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0]))
            .border(Quad::new(vec![1.0, 2.0, 11.0, 2.0, 11.0, 12.0, 1.0, 12.0]))
            .margin(Quad::new(vec![0.0, 1.0, 12.0, 1.0, 12.0, 13.0, 0.0, 13.0]))
            .width(10)
            .height(10)
            .build()
            .expect("box model should build");

        let bbox = box_model_to_bounding_box(&model).expect("bbox should exist");

        assert_eq!(bbox.x, 1.0);
        assert_eq!(bbox.y, 2.0);
        assert_eq!(bbox.width, 10.0);
        assert_eq!(bbox.height, 10.0);
    }

    #[test]
    fn node_to_inspect_info_flattens_attributes() {
        let node = Node::builder()
            .node_id(NodeId::new(1))
            .backend_node_id(BackendNodeId::new(2))
            .node_type(1)
            .node_name("DIV")
            .local_name("div")
            .node_value("")
            .attributes(["id", "app", "class", "hero"].into_iter().map(String::from))
            .build()
            .expect("node should build");

        let info = node_to_inspect_info("page".to_string(), node, "<div></div>".to_string(), None);

        assert_eq!(info.node_id, 1);
        assert_eq!(info.backend_node_id, 2);
        assert_eq!(info.attributes.get("id"), Some(&"app".to_string()));
        assert_eq!(info.attributes.get("class"), Some(&"hero".to_string()));
    }
}
