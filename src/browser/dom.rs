use super::{
    actor::BrowserActor,
    types::{BrowserValue, ClickKind, Locator},
    util::{js_locator_function, locator_name},
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
