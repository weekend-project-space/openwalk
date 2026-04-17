use super::{actor::BrowserActor, types::BrowserValue, *};

impl BrowserActor {
    pub(super) async fn mouse_move(&mut self, x: f64, y: f64) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        page.move_mouse(Point::new(x, y))
            .await
            .with_context(|| format!("failed to move mouse to ({x}, {y})"))?;
        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn mouse_click(&mut self, x: f64, y: f64, count: i64) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        page.click_with(
            Point::new(x, y),
            ClickOptions::builder().click_count(count).build(),
        )
        .await
        .with_context(|| format!("failed to click mouse at ({x}, {y})"))?;
        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn mouse_down(
        &mut self,
        x: f64,
        y: f64,
        button: MouseButton,
    ) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        page.move_mouse(Point::new(x, y)).await?;
        page.execute(
            DispatchMouseEventParams::builder()
                .r#type(DispatchMouseEventType::MousePressed)
                .x(x)
                .y(y)
                .button(button)
                .click_count(1)
                .build()
                .expect("mouse down should build"),
        )
        .await
        .with_context(|| format!("failed to press mouse at ({x}, {y})"))?;
        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn mouse_up(
        &mut self,
        x: f64,
        y: f64,
        button: MouseButton,
    ) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        page.execute(
            DispatchMouseEventParams::builder()
                .r#type(DispatchMouseEventType::MouseReleased)
                .x(x)
                .y(y)
                .button(button)
                .click_count(1)
                .build()
                .expect("mouse up should build"),
        )
        .await
        .with_context(|| format!("failed to release mouse at ({x}, {y})"))?;
        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn mouse_wheel(
        &mut self,
        x: i64,
        y: i64,
        delta_x: f64,
        delta_y: f64,
    ) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        page.execute(
            SynthesizeScrollGestureParams::builder()
                .x(x as f64)
                .y(y as f64)
                .x_distance(-delta_x)
                .y_distance(-delta_y)
                .build()
                .map_err(anyhow::Error::msg)?,
        )
        .await
        .context("failed to dispatch mouse wheel gesture")?;
        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn touch_tap(&mut self, x: i64, y: i64) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        page.execute(
            EmulateTouchFromMouseEventParams::builder()
                .r#type(EmulateTouchFromMouseEventType::MousePressed)
                .x(x)
                .y(y)
                .button(MouseButton::Left)
                .click_count(1)
                .build()
                .map_err(anyhow::Error::msg)?,
        )
        .await
        .context("failed to dispatch touch press")?;
        page.execute(
            EmulateTouchFromMouseEventParams::builder()
                .r#type(EmulateTouchFromMouseEventType::MouseReleased)
                .x(x)
                .y(y)
                .button(MouseButton::Left)
                .click_count(1)
                .build()
                .map_err(anyhow::Error::msg)?,
        )
        .await
        .context("failed to dispatch touch release")?;
        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn keyboard_type(&mut self, text: String) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        page.find_element("body")
            .await
            .context("failed to find page body before typing")?
            .type_str(text.as_str())
            .await
            .with_context(|| format!("failed to type text `{text}`"))?;
        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn keyboard_press(&mut self, key: String) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        page.find_element("body")
            .await
            .context("failed to find page body before pressing a key")?
            .press_key(key.as_str())
            .await
            .with_context(|| format!("failed to press key `{key}`"))?;
        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn keyboard_down(&mut self, key: String) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        let definition =
            get_key_definition(key.as_str()).ok_or_else(|| anyhow!("unknown key `{key}`"))?;
        let mut builder = DispatchKeyEventParams::builder()
            .r#type(if definition.text.is_some() || definition.key.len() == 1 {
                DispatchKeyEventType::KeyDown
            } else {
                DispatchKeyEventType::RawKeyDown
            })
            .key(definition.key)
            .code(definition.code)
            .windows_virtual_key_code(definition.key_code)
            .native_virtual_key_code(definition.key_code);
        if let Some(text) = definition.text {
            builder = builder.text(text);
        } else if definition.key.len() == 1 {
            builder = builder.text(definition.key);
        }
        page.execute(builder.build().map_err(anyhow::Error::msg)?)
            .await
            .with_context(|| format!("failed to press key down `{key}`"))?;
        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn keyboard_up(&mut self, key: String) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        let definition =
            get_key_definition(key.as_str()).ok_or_else(|| anyhow!("unknown key `{key}`"))?;
        page.execute(
            DispatchKeyEventParams::builder()
                .r#type(DispatchKeyEventType::KeyUp)
                .key(definition.key)
                .code(definition.code)
                .windows_virtual_key_code(definition.key_code)
                .native_virtual_key_code(definition.key_code)
                .build()
                .map_err(anyhow::Error::msg)?,
        )
        .await
        .with_context(|| format!("failed to release key `{key}`"))?;
        Ok(BrowserValue::Boolean(true))
    }
}
