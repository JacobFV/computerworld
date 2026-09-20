use super::*;

#[test]
fn prelude_loads_and_console_works() {
    let out = eval_logs("<p id=p>hi</p>", "console.log(typeof window, typeof document, document.getElementById('p').textContent);");
    assert_eq!(out, "object object hi");
}

#[test]
fn dom_mutation_is_visible_to_serialization() {
    let r = run("<div id=a></div>", "const d = document.getElementById('a'); const s = document.createElement('span'); s.textContent = 'x'; s.className = 'c'; d.appendChild(s);");
    assert!(errors(&r).is_empty(), "{}", errors(&r));
    assert_eq!(body_html(&r), "<div id=\"a\"><span class=\"c\">x</span></div><script>const d = document.getElementById('a'); const s = document.createElement('span'); s.textContent = 'x'; s.className = 'c'; d.appendChild(s);</script>");
}
