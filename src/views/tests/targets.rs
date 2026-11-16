use crate::app::{App, FilterFocus, Tab};
use crate::cache::Ticket;
use crate::mouse::Target;
use crate::views::filters;
use ratatui::{backend::TestBackend, layout::Rect, Terminal};

#[test]
fn filters_pin_their_sidebar_and_results_targets_to_exact_cells() {
    let config: crate::config::AppConfig = toml::from_str(
        "[jira]\nproject = 'DEMO'\nteam_name = 'Demo'\n\
         [[filters]]\nname = 'Mine'\njql = 'x'\n[[filters]]\nname = 'Bugs'\njql = 'y'\n",
    )
    .unwrap();
    let mut app = App::new();
    app.loading = false;
    app.active_tab = Tab::Filters;
    app.filter_focus = FilterFocus::Results;
    let mut parent = Ticket::for_test("DEMO-1", "In Progress");
    parent.labels = vec!["a".into()];
    let mut child = Ticket::for_test("DEMO-2", "In Progress");
    child.parent_key = Some("DEMO-1".into());
    child.labels = vec!["b".into()];
    app.filter_results = vec![parent, child, Ticket::for_test("DEMO-3", "To Do")];
    let targets = |app: &App| {
        app.mouse_targets.borrow_mut().clear();
        let mut terminal = Terminal::new(TestBackend::new(160, 30)).unwrap();
        terminal
            .draw(|f| filters::render(f, f.area(), app, &config, None))
            .unwrap();
        app.mouse_targets.borrow().clone()
    };
    // The sidebar is 40 columns and the results' divider one, so the results start at column 41.
    let sidebar = [
        (Rect::new(0, 2, 40, 1), Target::Filter(0)),
        (Rect::new(0, 3, 40, 1), Target::Filter(1)),
    ];
    // Rows: header 0 (In Progress), DEMO-1 (a parent) 1, its sub-task 2, header 3, DEMO-3 4.
    let results = |lead: u16, fold_parent: u16| {
        let mut cells = Vec::new();
        for (index, y) in [(0, 3), (1, 4), (2, 5), (3, 7), (4, 8)] {
            cells.push((Rect::new(41, y, 119, 1), Target::Row(index)));
            cells.push((Rect::new(41, y, lead, 1), Target::Mark(index)));
            match index {
                0 | 3 => cells.push((Rect::new(42, y, 1, 1), Target::Fold(index))),
                1 => cells.push((Rect::new(fold_parent, y, 1, 1), Target::Fold(index))),
                _ => {}
            }
        }
        cells
    };
    assert_eq!(
        targets(&app),
        [sidebar.to_vec(), results(3, 51)].concat(),
        "nothing marked: a three cell mark target, the arrow after the key"
    );
    app.selected_ticket_keys.insert("DEMO-3".into());
    assert_eq!(
        targets(&app),
        [sidebar.to_vec(), results(5, 53)].concat(),
        "marking: the checkbox's two cells are part of the mark target"
    );
}
