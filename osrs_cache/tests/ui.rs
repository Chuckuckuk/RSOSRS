//! UI renderer against the locally installed cache (skipped when absent).

use osrs_cache::interface::Component;
use osrs_cache::ui::{self, hit_at, standard_state, Assets, CacheAssets, Layout, Override, Rect, TopLevel};
use osrs_cache::*;

fn alpha(c: &ui::Canvas, x: i32, y: i32) -> u8 {
    c.rgba[((y * c.width + x) * 4 + 3) as usize]
}

#[test]
fn classic_layout_boxes_and_hits() {
    let Ok(cache) = Cache::open_default() else { return };
    let state = standard_state(&cache, Layout::Classic, Some(3)).unwrap();
    let mut assets = CacheAssets::new(&cache).unwrap();
    let (w, h) = (1000, 700);
    let (canvas, hits) = ui::draw(&state, &mut assets, w, h);

    // side panel background fills the 241x335 box at the bottom-right
    let side = Rect::new(w - 241, h - 335, 241, 335);
    assert!(alpha(&canvas, side.x + 120, side.y + 150) == 255, "panel background");
    assert_eq!(alpha(&canvas, side.x - 2, side.y + 150), 0, "nothing left of the panel");
    // chatbox in the bottom-left 519x165
    let chat = Rect::new(0, h - 165, 519, 165);
    assert_eq!(alpha(&canvas, chat.x + 200, chat.y + 60), 255, "chat background");
    assert_eq!(alpha(&canvas, chat.x + 525, chat.y + 60), 0);
    assert_eq!(alpha(&canvas, 300, chat.y - 3), 0, "viewport stays transparent");
    // the viewport hit covers the whole window
    assert!(hits.iter().any(|x| x.content_type == 1337 && x.rect == Rect::new(0, 0, w, h)));

    // stones: the topmost interactive hit at a stone's centre is the stone itself
    let t = TopLevel::of(Layout::Classic);
    for i in 0..14 {
        let st = hits.iter().find(|x| x.id == t.stone[i]).expect("stone has a hit");
        let (cx, cy) = (st.rect.x + st.rect.w / 2, st.rect.y + st.rect.h / 2);
        assert_eq!(hit_at(&hits, cx, cy).map(|x| x.id), Some(t.stone[i]), "stone {i}");
    }
    // the selected pane is shown, the others are skipped; the pane group is mounted
    assert!(hits.iter().any(|x| x.id == 149 << 16));
    assert!(!hits.iter().any(|x| x.id == 541 << 16));
}

#[test]
fn modern_and_fixed_draw() {
    let Ok(cache) = Cache::open_default() else { return };
    let mut assets = CacheAssets::new(&cache).unwrap();
    for (layout, w, h) in [(Layout::Modern, 1000, 700), (Layout::Fixed, 765, 503)] {
        let state = standard_state(&cache, layout, Some(0)).unwrap();
        let (canvas, hits) = ui::draw(&state, &mut assets, w, h);
        assert!(alpha(&canvas, w - 5, h - 5) == 255 || layout == Layout::Fixed);
        let t = TopLevel::of(layout);
        let st = hits.iter().find(|x| x.id == t.stone[3]).unwrap();
        assert_eq!(hit_at(&hits, st.rect.x + 5, st.rect.y + 5).map(|x| x.id), Some(t.stone[3]));
    }
}

#[test]
fn text_and_items() {
    let Ok(cache) = Cache::open_default() else { return };
    let mut assets = CacheAssets::new(&cache).unwrap();
    let font = assets.font(495).expect("font 495 loads");
    assert!(font.height > 8 && font.width("Hello") > 10);
    let mut state = ui::UiState { top: 1, ..Default::default() };
    let root = Component { id: 1 << 16, group: 1, is_if3: true, kind: 0, width_mode: 1, height_mode: 1, ..Default::default() };
    state.groups.insert(1, vec![root]);
    let mut c = Component { id: 1 << 16 | 1, group: 1, file: 1, is_if3: true, kind: 5, width: 36, height: 32, x: 10, y: 10, sprite_id: -1, ..Default::default() };
    c.parent = Some(1 << 16);
    state.groups.get_mut(&1).unwrap().push(c);
    state.overrides.insert(1 << 16 | 1, Override { item: Some((995, 250_000)), ..Default::default() });
    let (canvas, _) = ui::draw(&state, &mut assets, 100, 100);
    let any = (10..46).any(|x| (10..42).any(|y| alpha(&canvas, x, y) > 0));
    assert!(any, "coins icon and stack text drawn");
}
