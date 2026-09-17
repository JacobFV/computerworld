# Desktop overhaul drawing contract

Platform modules import `super::shared::{Painter,ShellContext,WindowView}` and `cw_scene::{Color,Rect,Primitive}`.
Each exports `background(&mut Painter,&ShellContext)`, `chrome(&mut Painter,&ShellContext)`, and `window_frame(&mut Painter,&ShellContext,&WindowView)`.
Background first; windows paint bottom-to-top with content clips; chrome last, always above windows. `ctx.windows` contains taskbar data. `ctx.active` indicates any visible window; `ctx.title` active title.

`WindowView`: id:u64, title:String, kind:String, rect:Rect, focused:bool, maximized:bool, minimized:bool, content:Option<Scene>. `w.action("drag")` yields `window:<id>:drag`. Controls use `close`, `minimize`, `maximize`; all drag/resize dispatch is Rust. Compositor installs resize regions. Frame implementations install draggable titlebar before control buttons. Content actions become `window:<id>:content:<original>`.

Central `work_area(theme,width,height)` and `window_content_rect(theme,frame)` keep layout identical to kernel pointer geometry. Frame titlebar heights macOS 42, Windows 38, Ubuntu 46; mobile top inset 96 and bottom inset 32. Native browser content includes browser navigation bar owned by compositor, 40px above site content.

Painter public fields scene,next,z. Methods: node(Rect,Primitive,Option<(&str,&str)>), box_(Rect,Color,radius), border(Rect,Color,radius,Color), text(x,y,width,&str,size,Color), button(Rect,Color,radius,action,label), region(Rect,action,label), path(points,fill), line(points,color,thickness), shadow(Rect,radius), asset(Rect,id), platform_icon(Rect,platform,kind,action,label), icon(x,y,size,kind,label). Asset ids `wallpaper/<platform>` and `icon/<platform>/<kind>` with platform macos/windows/ubuntu/ios/android; kind browser/files/terminal/editor/settings/mail/calendar/photos/camera/messages/phone/store. Shared icon fallback `icon/common/<kind>`. Asset resolver must provide supported keys; shells never embed RGBA.

`render_desktop(theme,width,height,clock_us,launcher_open,Vec<WindowView>)->Scene`. Legacy render_shell/content_rect remain wrappers during transition. No host time, randomness or filesystem in scene construction.

Options extension: `ShellOptions { installed_apps: Vec<String>, panel: Option<String>, search: String }`; `render_desktop_with_options(..., windows, options)`. Context borrows these fields and `ctx.installed(id)` filters installed app IDs; labels are OS-specific. Per-OS modules own visual panels; environment shell_extensions owns their semantics. `Painter.bold(...)` uses bundled bold text, `shadow(...)` uses cached soft rounded shadow primitive. `window_content_rect_for_kind(theme,frame,"browser")` additionally reserves a 40px real browser navigation bar. Browser actions use per-window content namespace and page content remains clipped below toolbar.

Pointer feedback: ShellOptions and ShellContext expose `hover: Option<(i32,i32)>` in global scene coordinates. `ctx.hovered(Rect)` performs deterministic hit containment. Pointer positions are simulation/interface state; platform modules draw hover controls without host CSS semantics.
