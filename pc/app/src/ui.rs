//! egui + wgpu 的界面：把最新一帧上传到离屏纹理、转成 RGB、画到窗口里。
//!
//! 界面很薄 —— 视频面 + 状态栏 + 日志尾巴。**真正的活儿都在会话线程**，
//! 这里只做两件事：把 [`Handles`] 里的最新帧喂给 GPU，以及把状态显示出来。
//!
//! 帧通路的落点（与 spike 实测一致）：
//! `会话线程解码 → 回读 NV12 → （这里）上传两张平面纹理 → 着色器转 RGB → egui 采样`。

use std::time::Instant;

use decode::{Nv12Pipeline, StreamTarget};
use eframe::{egui, egui_wgpu, wgpu};

use crate::input::{self, Sampler};
use crate::session::{Handles, Phase, Session};

/// 应用状态。
pub struct App {
    /// 会话（`on_exit` 时收尾）。
    session: Session,
    /// 与会话线程共享的句柄。
    handles: Handles,
    /// egui 的 wgpu 渲染状态（含 `device` / `queue` / `renderer`）。
    rs: egui_wgpu::RenderState,
    /// 共享的 NV12 → RGB 管线。
    pipe: Nv12Pipeline,
    /// 当前视频的纹理与绑定组（分辨率变化时重建）。
    target: Option<StreamTarget>,
    /// 交给 egui 采样的纹理 id（重建时必须先释放，否则泄漏）。
    tex_id: Option<egui::TextureId>,
    /// 输入采样器（鼠标 / 键盘 → `input` 指令，规则见 [`crate::input`]）。
    sampler: Sampler,
    /// 采样上限是否已按 agent 回报的刷新率设过（只设一次）。
    rate_set: bool,
    /// 已上屏的帧数。
    shown: u64,
    /// 窗口内实测的显示帧率。
    fps: f32,
    /// 本统计窗口内上屏的帧数。
    fps_count: u32,
    /// 本统计窗口的起点。
    fps_window: Instant,
}

impl App {
    /// 建应用。
    ///
    /// @param cc eframe 的创建上下文
    /// @param session 已经建立好的会话（连不上时**根本走不到这里** —— 那是在 `main` 里
    ///   就明确失败并退出的路径）
    pub fn new(cc: &eframe::CreationContext<'_>, session: Session) -> Self {
        let rs = cc
            .wgpu_render_state
            .clone()
            .expect("WCReen 必须跑在 wgpu 后端上（main 里已指定 Renderer::Wgpu）");
        install_cjk_font(&cc.egui_ctx);
        let pipe = Nv12Pipeline::new(&rs.device);
        // 会话线程靠这个回调叫醒 UI —— 否则它睡下去就再没人叫醒（spike 踩过的坑）。
        let ctx = cc.egui_ctx.clone();
        session.waker().set(move || ctx.request_repaint());
        Self {
            handles: session.handles().clone(),
            session,
            rs,
            pipe,
            target: None,
            tex_id: None,
            sampler: Sampler::new(input::DEFAULT_RATE_HZ),
            rate_set: false,
            shown: 0,
            fps: 0.0,
            fps_count: 0,
            fps_window: Instant::now(),
        }
    }

    /// 把最新一帧送上屏（分辨率变化时重建纹理）。
    fn present(&mut self) {
        let Some(frame) = self.handles.take_frame() else {
            return;
        };
        let need_rebuild = self
            .target
            .as_ref()
            .is_none_or(|t| !t.matches(&frame));
        if need_rebuild {
            // 先把旧的纹理 id 还给 egui，再换新的 —— 否则每换一次分辨率就泄漏一个纹理。
            if let Some(id) = self.tex_id.take() {
                self.rs.renderer.write().free_texture(&id);
            }
            let target = StreamTarget::new(&self.rs.device, &self.pipe, frame.width, frame.height);
            self.tex_id = Some(self.rs.renderer.write().register_native_texture(
                &self.rs.device,
                &target.view,
                wgpu::FilterMode::Linear,
            ));
            self.target = Some(target);
        }
        if let Some(target) = &self.target {
            target.upload(&self.rs.queue, &frame);
            target.render(&self.rs.device, &self.rs.queue, &self.pipe);
        }
        self.shown += 1;
        self.fps_count += 1;
        let elapsed = self.fps_window.elapsed().as_secs_f32();
        if elapsed >= 1.0 {
            self.fps = self.fps_count as f32 / elapsed;
            self.fps_count = 0;
            self.fps_window = Instant::now();
        }
    }

    /// 状态栏。
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let status = self.handles.status();
        let (phase, color) = match status.phase {
            Phase::Streaming => ("收帧中", egui::Color32::LIGHT_GREEN),
            Phase::Reconnecting => ("重连中", egui::Color32::YELLOW),
            Phase::Failed => ("已失败", egui::Color32::LIGHT_RED),
            Phase::Stopped => ("已结束", egui::Color32::GRAY),
        };
        ui.horizontal(|ui| {
            ui.colored_label(color, phase);
            ui.separator();
            let (codec, size) = match &status.applied {
                Some(a) => (
                    a.codec_name.clone().unwrap_or_else(|| "?".into()),
                    format!("{}x{}", a.width.unwrap_or(0), a.height.unwrap_or(0)),
                ),
                None => ("?".into(), "?".into()),
            };
            ui.monospace(format!(
                "streamId={}  {codec}  {size}  @{:.0}fps",
                status.stream_id,
                status.applied.as_ref().and_then(|a| a.refresh_rate).unwrap_or(0.0)
            ));
            ui.separator();
            ui.monospace(format!(
                "显示 {:.1} fps  已上屏 {}  解出 {}  丢弃 {}",
                self.fps, self.shown, status.frames, status.dropped
            ));
            ui.separator();
            ui.monospace(format!("输入 已发 {}", status.input_sent));
            if status.input_rejected > 0 {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    format!("被拒 {}", status.input_rejected),
                );
            }
        });
        if status.input_blocked {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                "输入不可用：agent 报告输入子系统不可用（多为未开启「USB 调试（安全设置）」）",
            );
        }
        if !status.message.is_empty() {
            ui.monospace(&status.message);
        }
    }

    /// 日志尾巴。
    fn log_tail(&self, ui: &mut egui::Ui) {
        let lines = self.handles.log_lines();
        for line in lines.iter().rev().take(6).collect::<Vec<_>>().into_iter().rev() {
            ui.monospace(line);
        }
    }
}

/// 给 egui 装一个中文字体。
///
/// egui 自带字体**没有 CJK 字形** ⇒ 界面里的中文会渲染成方框（spike 因此把界面文字
/// 全写成 ASCII）。本项目的界面与日志都是中文，所以这里按平台候选表找一个系统字体
/// 作为**兜底**（追加到字体族末尾 ⇒ 默认字体找不到的字形才用它）。
///
/// 找不到就**静默跳过**：界面主体仍是可读的 ASCII 计数与状态，缺字体不影响可用性，
/// 而且这条路径不该让程序起不来。
///
/// @param ctx egui 上下文
fn install_cjk_font(ctx: &egui::Context) {
    /// 各平台常见的中文字体（按优先级）。
    const CANDIDATES: &[&str] = &[
        // Windows
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
        // macOS
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        // Linux
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    ];

    for path in CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if !looks_like_font(&bytes) {
            continue;
        }
        let mut fonts = egui::FontDefinitions::default();
        fonts
            .font_data
            .insert("cjk".to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts.families.entry(family).or_default().push("cjk".to_owned());
        }
        ctx.set_fonts(fonts);
        return;
    }
}

/// 粗略校验文件确实是字体 —— 避免把一个损坏 / 非字体文件喂给 epaint（那会 panic）。
///
/// 只认四种魔数：`ttcf`（字体集合）、`OTTO`（CFF）、`true`（旧式 TTF）、`0x00010000`（TTF）。
///
/// @param bytes 文件内容
fn looks_like_font(bytes: &[u8]) -> bool {
    match bytes.first_chunk::<4>() {
        Some([b't', b't', b'c', b'f']) => true,
        Some([b'O', b'T', b'T', b'O']) => true,
        Some([b't', b'r', b'u', b'e']) => true,
        Some([0x00, 0x01, 0x00, 0x00]) => true,
        _ => false,
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // ① 先把新帧送上屏（GPU 侧：上传平面纹理 → 着色器转 RGB → 离屏纹理）
        self.present();

        // ② 采样上限 = agent 回报的**实际刷新率**（拿到之前先用默认值）。
        //    「高于屏刷新率的 move 没有可见价值」（`tech-stack/pc.md` §4.2）。
        if !self.rate_set {
            let hz = self.handles.status().applied.as_ref().and_then(|a| a.refresh_rate);
            if let Some(hz) = hz.filter(|v| *v > 0.0) {
                self.sampler.set_rate(hz as f32);
                self.rate_set = true;
            }
        }

        // ③ 状态栏
        egui::Panel::top("status").show(ui, |ui| self.status_bar(ui));

        // ④ 底部日志
        egui::Panel::bottom("log").show(ui, |ui| self.log_tail(ui));

        // ⑤ 视频面 + 输入采集
        let input_blocked = self.handles.status().input_blocked;
        egui::CentralPanel::no_frame().show(ui, |ui| {
            let (Some(id), Some(target)) = (self.tex_id, &self.target) else {
                ui.centered_and_justified(|ui| {
                    ui.label("等待第一帧…");
                });
                return;
            };
            // 等比缩放铺满可用区域。**必须在进入布局前算好尺寸** ——
            // 布局内部的 `available_height()` 会退化成 0，会把画面压成一条线（spike 的坑）。
            let avail = ui.available_size();
            let [w, h] = target.size();
            let scale = (avail.x / w).min(avail.y / h).max(0.01);
            let size = egui::vec2(w * scale, h * scale);
            // 自己算居中的矩形再 `put` —— 这样 `response.rect` 就是**画面本身**，
            // 「指针坐标 → 屏像素」不需要猜任何偏移。而屏像素就是 PC 鼠标该报的坐标：
            // 编码尺寸 = 屏尺寸（`agent/display.md` 的硬约束）⇒ 中间没有缩放 ✗ 无换算误差 ✓
            let rect = egui::Rect::from_center_size(ui.available_rect_before_wrap().center(), size);
            let response = ui
                .put(rect, egui::Image::new((id, size)))
                .interact(egui::Sense::click_and_drag());

            if input_blocked {
                return;
            }

            // 光标 → 屏像素（夹在画面内：拖到窗口外按边缘算，与 scrcpy 同做法）
            let mut at = None;
            if let Some(pos) = response.hover_pos().or_else(|| response.interact_pointer_pos()) {
                let x = ((pos.x - rect.min.x) / scale).clamp(0.0, w - 1.0) as i32;
                let y = ((pos.y - rect.min.y) / scale).clamp(0.0, h - 1.0) as i32;
                self.sampler.cursor(x, y);
                at = Some((x, y));
            }

            // 触点按下 / 抬起 —— **不能只认 drag**：egui 的 `drag_started` / `drag_stopped`
            // 只在"确实在拖"（按下够久或移动够远）时才为真，**纯点击一路都不触发** ✗
            // （egui 文档原话：鼠标按下 ≠ 正在拖动，可能只是点击的开头）。
            // 实测症状：拖动有效、点击完全没反应。
            //
            // 改成**事件驱动**，不带任何阈值判定：
            //   主键按下 **且落在画面上** ⇒ 触点按下；
            //   主键抬起 **且我们这边正按着** ⇒ 触点抬起（抬在哪里都算，与 scrcpy 同）。
            // 只用主键，所以右键 / 中键（BACK / HOME）不会产生触点。
            let pressed_here =
                ui.input(|i| i.pointer.primary_pressed()) && response.contains_pointer();
            let released = ui.input(|i| i.pointer.primary_released());
            if pressed_here && !self.sampler.is_dragging() {
                self.sampler.press();
            } else if released && self.sampler.is_dragging() {
                self.sampler.release();
            }

            // 右键 = BACK、中键 = HOME（都是"按一下"的按键，不参与触点状态）
            if response.clicked_by(egui::PointerButton::Secondary) {
                self.sampler.key(input::keycode::BACK, 0, true);
                self.sampler.key(input::keycode::BACK, 0, false);
            }
            if response.clicked_by(egui::PointerButton::Middle) {
                self.sampler.key(input::keycode::HOME, 0, true);
                self.sampler.key(input::keycode::HOME, 0, false);
            }

            // 滚轮：**累加**，绝不丢（增量语义 —— `tech-stack/pc.md` §4.1）
            //
            // ⚠️ 符号要**取负**：egui 的 `smooth_scroll_delta` 正 = **内容向下移**
            //（即在触摸屏上往下拖，看到更早的内容）；而 Android 的 `AXIS_VSCROLL`
            // 经 `View.scrollBy(0, +)` 把**视口**往下移 ⇒ 正 = 看到更晚的内容。
            // 两者同一个方向、符号相反 ⇒ 取负才符合"轮子往回转 = 看后面"的直觉。
            if response.hovered() {
                let delta = ui.input(|i| i.smooth_scroll_delta);
                if delta.x != 0.0 || delta.y != 0.0 {
                    let (x, y) = at.or_else(|| self.sampler.cursor_pos()).unwrap_or((0, 0));
                    self.sampler.scroll(-(delta.x as i32), -(delta.y as i32), x, y);
                }
            }
        });

        // ⑥ 键盘：窗口**聚焦时**才发（否则会往屏上打出用户没按的键）。
        //    长按由重复的 `keyDown` 表达 —— agent 侧的 `repeat` 恒为 0。
        if ui.input(|i| i.focused) {
            let events = ui.input(|i| i.events.clone());
            for event in events {
                if let egui::Event::Key { key, pressed, modifiers, .. } = event {
                    if let Some(code) = input::android_keycode(key) {
                        self.sampler.key(code, input::android_meta(&modifiers), pressed);
                    }
                }
            }
        } else {
            // 失焦：必须取消手势，否则 agent 侧那个触点**永远按着** ✗
            self.sampler.cancel();
        }
        // 在窗口外松开鼠标时上面那条路可能不再为真 ⇒ 按"指针是否还按着"兜底，
        // 免得 agent 侧那个触点永远停在按下态（`release()` 是幂等的）。
        if self.sampler.is_dragging() && !ui.input(|i| i.pointer.any_down()) {
            self.sampler.release();
        }

        // ⑦ 把攒下的输入投给会话线程（本线程不是写者），并保持刷新 ——
        //    没有新帧时状态栏的 fps / 阶段也会变（例如掉线重连）。
        let cmds = self.sampler.poll(Instant::now());
        self.handles.push_input(cmds);
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
    }

    fn on_exit(&mut self) {
        // 退出时发 stopAgent（协议要求）—— 这会等会话线程把消息发出去再收尾。
        self.session.stop();
    }
}
