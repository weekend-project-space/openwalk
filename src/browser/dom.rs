use std::{env, path::PathBuf};

use chromiumoxide::cdp::browser_protocol::dom::SetFileInputFilesParams;

use super::{
    actor::BrowserActor,
    types::{BrowserValue, ClickKind, Locator},
    util::{js_locator_function, json_to_browser_value, locator_name, serialize_to_browser_value},
    *,
};

impl BrowserActor {
    pub(super) async fn click_locator(
        &mut self,
        locator: Locator,
        click: ClickKind,
    ) -> Result<BrowserValue> {
        let element = self.find_locator(&locator).await?;
        match click {
            ClickKind::Single => {
                element.click().await.with_context(|| {
                    format!(
                        "failed to click {} `{}`",
                        locator_name(locator.kind()),
                        locator.raw()
                    )
                })?;
            }
            ClickKind::Double => {
                element
                    .click_with(ClickOptions::builder().click_count(2).build())
                    .await
                    .with_context(|| {
                        format!(
                            "failed to double click {} `{}`",
                            locator_name(locator.kind()),
                            locator.raw()
                        )
                    })?;
            }
            ClickKind::Right => {
                let page = self.require_page_ready().await?;
                let point = element
                    .scroll_into_view()
                    .await
                    .with_context(|| {
                        format!(
                            "failed to scroll {} `{}` into view",
                            locator_name(locator.kind()),
                            locator.raw()
                        )
                    })?
                    .clickable_point()
                    .await
                    .with_context(|| {
                        format!(
                            "failed to resolve clickable point for {} `{}`",
                            locator_name(locator.kind()),
                            locator.raw()
                        )
                    })?;
                page.move_mouse(point).await?;
                page.execute(
                    DispatchMouseEventParams::builder()
                        .r#type(DispatchMouseEventType::MousePressed)
                        .x(point.x)
                        .y(point.y)
                        .button(MouseButton::Right)
                        .click_count(1)
                        .build()
                        .expect("mouse press should build"),
                )
                .await?;
                page.execute(
                    DispatchMouseEventParams::builder()
                        .r#type(DispatchMouseEventType::MouseReleased)
                        .x(point.x)
                        .y(point.y)
                        .button(MouseButton::Right)
                        .click_count(1)
                        .build()
                        .expect("mouse release should build"),
                )
                .await?;
            }
        }

        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn type_locator(
        &mut self,
        locator: Locator,
        text: String,
    ) -> Result<BrowserValue> {
        let element = self.find_locator(&locator).await?;
        element
            .click()
            .await
            .with_context(|| {
                format!(
                    "failed to focus {} `{}`",
                    locator_name(locator.kind()),
                    locator.raw()
                )
            })?
            .type_str(text.as_str())
            .await
            .with_context(|| {
                format!(
                    "failed to type into {} `{}`",
                    locator_name(locator.kind()),
                    locator.raw()
                )
            })?;
        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn fill_locator(
        &mut self,
        locator: Locator,
        text: String,
    ) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        self.find_locator(&locator)
            .await?
            .focus()
            .await
            .with_context(|| {
                format!(
                    "failed to focus {} `{}`",
                    locator_name(locator.kind()),
                    locator.raw()
                )
            })?;

        let expression = js_locator_function(
            &locator,
            format!(
                r#"
                el.value = {text:?};
                el.dispatchEvent(new Event("input", {{ bubbles: true }}));
                el.dispatchEvent(new Event("change", {{ bubbles: true }}));
                return el.value;
                "#
            ),
        );
        page.evaluate(expression.as_str()).await.with_context(|| {
            format!(
                "failed to fill {} `{}`",
                locator_name(locator.kind()),
                locator.raw()
            )
        })?;

        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn select_locator(
        &mut self,
        locator: Locator,
        value: String,
    ) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        let expression = js_locator_function(
            &locator,
            format!(
                r#"
                el.value = {value:?};
                el.dispatchEvent(new Event("input", {{ bubbles: true }}));
                el.dispatchEvent(new Event("change", {{ bubbles: true }}));
                return el.value;
                "#
            ),
        );
        page.evaluate(expression.as_str()).await.with_context(|| {
            format!(
                "failed to select value on {} `{}`",
                locator_name(locator.kind()),
                locator.raw()
            )
        })?;
        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn set_checked_locator(
        &mut self,
        locator: Locator,
        checked: bool,
    ) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        let expression = js_locator_function(
            &locator,
            format!(
                r#"
                el.checked = {checked};
                el.dispatchEvent(new Event("input", {{ bubbles: true }}));
                el.dispatchEvent(new Event("change", {{ bubbles: true }}));
                return !!el.checked;
                "#
            ),
        );
        page.evaluate(expression.as_str()).await.with_context(|| {
            format!(
                "failed to update checked state on {} `{}`",
                locator_name(locator.kind()),
                locator.raw()
            )
        })?;
        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn exists_locator(&mut self, locator: Locator) -> Result<BrowserValue> {
        Ok(BrowserValue::Boolean(
            self.find_locator(&locator).await.is_ok(),
        ))
    }

    pub(super) async fn hover_locator(&mut self, locator: Locator) -> Result<BrowserValue> {
        self.find_locator(&locator)
            .await?
            .hover()
            .await
            .with_context(|| {
                format!(
                    "failed to hover {} `{}`",
                    locator_name(locator.kind()),
                    locator.raw()
                )
            })?;
        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn upload_locator(
        &mut self,
        locator: Locator,
        files: Vec<String>,
    ) -> Result<BrowserValue> {
        let element = self.find_locator(&locator).await?;
        let page = self.require_page_ready().await?;
        let resolved_files = files
            .iter()
            .map(|value| resolve_upload_file_path(value))
            .collect::<Result<Vec<_>>>()?;
        page.execute(
            SetFileInputFilesParams::builder()
                .files(resolved_files.clone())
                .backend_node_id(element.backend_node_id)
                .build()
                .map_err(anyhow::Error::msg)?,
        )
        .await
        .with_context(|| {
            format!(
                "failed to upload files into {} `{}`",
                locator_name(locator.kind()),
                locator.raw()
            )
        })?;

        serialize_to_browser_value(&resolved_files, "failed to serialize uploaded file paths")
    }

    pub(super) async fn drag_between_locators(
        &mut self,
        source: Locator,
        target: Locator,
    ) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        let value: serde_json::Value = page
            .evaluate(js_drag_between_locators(&source, &target).as_str())
            .await
            .with_context(|| {
                format!(
                    "failed to drag {} `{}` to {} `{}`",
                    locator_name(source.kind()),
                    source.raw(),
                    locator_name(target.kind()),
                    target.raw()
                )
            })?
            .into_value()
            .context("drag operation returned a non-serializable value")?;
        Ok(json_to_browser_value(value))
    }

    pub(super) async fn find_locator(
        &mut self,
        locator: &Locator,
    ) -> Result<chromiumoxide::Element> {
        let page = self.require_page_ready().await?;
        match locator {
            Locator::Css(selector) => page
                .find_element(selector.as_str())
                .await
                .with_context(|| format!("failed to find selector `{selector}`")),
            Locator::XPath(xpath) => page
                .find_xpath(xpath.as_str())
                .await
                .with_context(|| format!("failed to find xpath `{xpath}`")),
        }
    }
}

fn resolve_upload_file_path(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!("upload file path cannot be empty");
    }

    let path = PathBuf::from(trimmed);
    let absolute = if path.is_absolute() {
        path
    } else {
        env::current_dir()
            .context("failed to resolve current working directory for upload")?
            .join(path)
    };
    let canonical = absolute
        .canonicalize()
        .with_context(|| format!("failed to resolve upload file `{trimmed}`"))?;
    if !canonical.is_file() {
        bail!("upload path `{}` is not a file", canonical.display());
    }
    Ok(canonical.display().to_string())
}

fn js_drag_between_locators(source: &Locator, target: &Locator) -> String {
    let source_expression = match source {
        Locator::Css(selector) => format!("document.querySelector({selector:?})"),
        Locator::XPath(xpath) => format!(
            r#"document.evaluate(
                {xpath:?},
                document,
                null,
                XPathResult.FIRST_ORDERED_NODE_TYPE,
                null
            ).singleNodeValue"#
        ),
    };
    let target_expression = match target {
        Locator::Css(selector) => format!("document.querySelector({selector:?})"),
        Locator::XPath(xpath) => format!(
            r#"document.evaluate(
                {xpath:?},
                document,
                null,
                XPathResult.FIRST_ORDERED_NODE_TYPE,
                null
            ).singleNodeValue"#
        ),
    };

    format!(
        r#"() => {{
            const source = {source_expression};
            if (!source) {{
                throw new Error("{source_name} `{source_raw}` not found");
            }}
            const target = {target_expression};
            if (!target) {{
                throw new Error("{target_name} `{target_raw}` not found");
            }}
            source.scrollIntoView({{ block: "center", inline: "center" }});
            target.scrollIntoView({{ block: "center", inline: "center" }});
            if (typeof DataTransfer !== "function" || typeof DragEvent !== "function") {{
                throw new Error("drag and drop is not supported in this browser context");
            }}

            const dataTransfer = new DataTransfer();
            dataTransfer.effectAllowed = "all";
            dataTransfer.dropEffect = "move";
            const fire = (element, type) =>
                element.dispatchEvent(
                    new DragEvent(type, {{
                        bubbles: true,
                        cancelable: true,
                        composed: true,
                        dataTransfer
                    }})
                );

            fire(source, "dragstart");
            fire(target, "dragenter");
            fire(target, "dragover");
            fire(target, "drop");
            fire(source, "dragend");
            return true;
        }}"#,
        source_expression = source_expression,
        source_name = locator_name(source.kind()),
        source_raw = source.raw(),
        target_expression = target_expression,
        target_name = locator_name(target.kind()),
        target_raw = target.raw(),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::resolve_upload_file_path;

    #[test]
    fn resolve_upload_file_path_returns_absolute_file_path() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos();
        let base = std::env::temp_dir().join(format!("openwalk-upload-test-{nonce}"));
        fs::create_dir_all(&base).expect("temp dir should exist");
        let file = base.join("sample.txt");
        fs::write(&file, "hello").expect("temp file should be written");

        let resolved =
            resolve_upload_file_path(file.to_str().expect("temp file path should be utf8"))
                .expect("upload file path should resolve");

        assert!(resolved.ends_with("sample.txt"));
        assert!(std::path::Path::new(&resolved).is_absolute());

        let _ = fs::remove_dir_all(&base);
    }
}
