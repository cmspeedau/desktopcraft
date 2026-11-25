//! Offline OLE->DesktopCraft staging gate (no HWND, Explorer hook or filesystem I/O).
//!
//! A future original-theme Box receiver can use this after independently
//! verifying its native HWND, exact CF_HDROP paths and physical window metrics.
//! Captures full State, native icon identities/positions and monitor geometry
//! at DragEnter. Commit is an in-memory compare-and-swap: no user Workspace
//! save, Explorer icon mutation, file operation or data import is performed.
use crate::explorer_native::{native_multi_box_plan, plan_native_ole_drop, NativeMultiBoxPlan};
use crate::model::{State, HEADER};
use crate::shell::ViewItem;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Measured HWND client bounds in *physical* screen pixels. DesktopCraft's
/// Group bounds are expressed in Macroquad logical UI units. The reported
/// logical viewport is from the same frame as the HWND measurement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeOleViewport {
    pub screen_origin_px: (i32, i32),
    pub client_size_px: (u32, u32),
    pub logical_size: (f64, f64),
}
impl NativeOleViewport {
    fn valid(&self) -> bool {
        let (w, h) = self.client_size_px;
        let (lw, lh) = self.logical_size;
        if w == 0
            || h == 0
            || w > 16384
            || h > 16384
            || !lw.is_finite()
            || !lh.is_finite()
            || lw <= 0.
            || lh <= 0.
        {
            return false;
        }
        let (sx, sy) = (w as f64 / lw, h as f64 / lh);
        // Explicitly fail on untrusted DPI units and distorted viewports.
        sx >= 0.5 && sx <= 4. && sy >= 0.5 && sy <= 4. && (sx - sy).abs() <= 0.02
    }

    /// Return the logical Box-local pointer in the original UI coordinate
    /// space, rejecting clicks in other HWNDs, other screens or outside bounds.
    pub fn logical_at(&self, screen: (i32, i32)) -> Result<(f64, f64), String> {
        if !self.valid() {
            return Err("OLE receiver has invalid physical/logical viewport geometry".into());
        }
        let x = i64::from(screen.0) - i64::from(self.screen_origin_px.0);
        let y = i64::from(screen.1) - i64::from(self.screen_origin_px.1);
        if x < 0
            || y < 0
            || x >= i64::from(self.client_size_px.0)
            || y >= i64::from(self.client_size_px.1)
        {
            return Err("OLE drop is outside the measured DesktopCraft client HWND".into());
        }
        Ok((
            x as f64 * self.logical_size.0 / f64::from(self.client_size_px.0),
            y as f64 * self.logical_size.1 / f64::from(self.client_size_px.1),
        ))
    }
}

fn native_identity_snapshot(explorer: &[ViewItem]) -> Vec<(String, String, (i32, i32))> {
    explorer
        .iter()
        .map(|it| (it.path.clone(), it.label.clone(), it.pos))
        .collect()
}

fn snapshot_state(state: &State) -> Result<Vec<u8>, String> {
    serde_json::to_vec(state).map_err(|e| format!("Cannot snapshot OLE Workspace: {e}"))
}

/// A single-consumption ticket for 1..=8 distinct native filesystem items
/// from one verified visible Source Tab. Created at DragEnter/DragOver
/// (after the receiver HWND is verified).
/// The full workspace and every observed native icon position must still
/// match when Drop is committed on the main UI thread.
pub struct NativeOleDropTicket {
    state_before: Vec<u8>,
    native_before: Vec<(String, String, (i32, i32))>,
    monitor_signature: String,
    viewport: NativeOleViewport,
    native_plan: NativeMultiBoxPlan,
    offered_paths: Vec<PathBuf>,
    visible_boxes: BTreeSet<u64>,
    created_at: Instant,
    source_group: u64,
    max_groups: usize,
    max_items_per_tab: usize,
}
impl NativeOleDropTicket {
    pub fn begin(
        state: &State,
        explorer: &[ViewItem],
        monitor_signature: &str,
        viewport: NativeOleViewport,
        offered_paths: &[PathBuf],
        visible_box_ids: &[u64],
        max_groups: usize,
        max_items_per_tab: usize,
    ) -> Result<Self, String> {
        if monitor_signature.trim().is_empty() || !viewport.valid() {
            return Err("OLE receiver monitor or viewport was not validated".into());
        }
        if offered_paths.is_empty() || offered_paths.len()>8 ||
            explorer.len()>2048 || explorer.is_empty() {
            return Err("OLE receiver requires 1..=8 unique native Desktop items".into());
        }
        let plan=native_multi_box_plan(state,explorer,max_groups,max_items_per_tab)?;
        let mut source:Option<(u64,usize)>=None;
        let mut seen=std::collections::BTreeSet::new();
        for offered in offered_paths {
            let path=offered.to_str()
                .filter(|s|!s.trim().is_empty())
                .ok_or("OLE source has no valid filesystem identity")?;
            if !seen.insert(path.to_lowercase()){
                return Err("OLE selection contains duplicate source identities".into());
            }
            let indices=explorer.iter().enumerate()
                .filter(|(_,it)|it.path.eq_ignore_ascii_case(path))
                .map(|(i,_)|i).collect::<Vec<_>>();
            let [index]=indices.as_slice() else{
                return Err("OLE selected item must have unique Explorer identity".into());
            };
            let claims=plan.groups.iter().filter_map(|g|
                g.native_indices.get(g.active)
                    .filter(|indices|indices.contains(index))
                    .map(|_|(g.group_id,g.active))
            ).collect::<Vec<_>>();
            let [claim]=claims.as_slice() else {
                return Err("OLE selection must belong to a verified active Box Tab".into());
            };
            if let Some(previous)=source {
                if previous!=*claim {
                    return Err("OLE selection spans different Box Tabs".into());
                }
            }else{source=Some(*claim);}
        }
        let source_group=source.ok_or("OLE selected source Box unavailable")?.0;
        let visible_boxes = visible_box_ids.iter().copied().collect::<BTreeSet<_>>();
        if visible_boxes.len() != visible_box_ids.len()
            || visible_boxes.len() < 2
            || !visible_boxes.contains(&source_group)
            || visible_boxes
                .iter()
                .any(|id| !plan.groups.iter().any(|g| g.group_id == *id))
        {
            return Err("OLE receiver requires a unique, verified list of visible Box IDs".into());
        }
        Ok(Self {
            state_before: snapshot_state(state)?,
            native_before: native_identity_snapshot(explorer),
            monitor_signature: monitor_signature.into(),
            viewport,
            native_plan: plan,
            offered_paths: offered_paths.to_vec(),
            visible_boxes,
            created_at: Instant::now(),
            source_group,
            max_groups,
            max_items_per_tab,
        })
    }

    /// Hit-test a point in physical screen coordinates against only the
    /// currently active, fully verified Box bodies. Headers, resize handles,
    /// rolled/portal Boxes, the source Box and overlapping targets fail closed.
    pub fn target_at(&self, state: &State, screen: (i32, i32)) -> Result<(u64, u64), String> {
        let (x, y) = self.viewport.logical_at(screen)?;
        let mut matches = Vec::new();
        for plan in &self.native_plan.groups {
            if plan.group_id == self.source_group || !self.visible_boxes.contains(&plan.group_id) {
                continue;
            }
            let group = state
                .group(plan.group_id)
                .ok_or("OLE target Box was removed")?;
            if group.rolled
                || group.page != state.page
                || group.active != plan.active
                || group.active >= group.tabs.len()
                || group.tabs[group.active].folder.is_some()
            {
                continue;
            }
            let b = group.bounds;
            let (l, t, w, h) = (
                f64::from(b.x),
                f64::from(b.y),
                f64::from(b.w),
                f64::from(b.h),
            );
            if ![l, t, w, h].iter().all(|v| v.is_finite())
                || w < 100.
                || h < f64::from(HEADER) + 32.
            {
                continue;
            }
            let header = f64::from(HEADER) + 4.;
            let inset = 8.;
            if x >= l + inset && x < l + w - inset && y >= t + header && y < t + h - inset {
                matches.push((group.id, group.tabs[group.active].id));
            }
        }
        match matches.as_slice() {
            [only] => Ok(*only),
            [] => Err("OLE pointer outside any single verified Box body".into()),
            _ => Err("Overlapping OLE Box hit zones are ambiguous".into()),
        }
    }

    fn validate_current(
        &self,
        state: &State,
        explorer: &[ViewItem],
        monitor_signature: &str,
        viewport: NativeOleViewport,
        live_visible_box_ids: &[u64],
        received_paths: &[PathBuf],
    ) -> Result<(), String> {
        if received_paths != self.offered_paths {
            return Err("OLE CF_HDROP source changed between DragEnter and Drop".into());
        }
        if self.created_at.elapsed() > Duration::from_secs(90) {
            return Err("OLE drop ticket expired; begin a new gesture".into());
        }
        if monitor_signature != self.monitor_signature || viewport != self.viewport {
            return Err("OLE display topology, DPI or Window geometry changed".into());
        }
        let live_visible = live_visible_box_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if live_visible.len() != live_visible_box_ids.len() || live_visible != self.visible_boxes {
            return Err("OLE Box visibility changed during drag".into());
        }
        if snapshot_state(state)? != self.state_before {
            return Err("OLE Workspace changed during drag; transaction cancelled".into());
        }
        if native_identity_snapshot(explorer) != self.native_before {
            return Err("OLE Explorer native identities/order/coordinates changed".into());
        }
        if native_multi_box_plan(state, explorer, self.max_groups, self.max_items_per_tab)?
            != self.native_plan
        {
            return Err("OLE native Box policy changed during drag".into());
        }
        Ok(())
    }

    /// Dry-run every commit condition to decide whether to advertise a LINK
    /// over a given Box. Still does not modify the workspace or Explorer.
    pub fn preview(
        &self,
        live: &State,
        explorer: &[ViewItem],
        monitor_signature: &str,
        viewport: NativeOleViewport,
        live_visible_box_ids: &[u64],
        received_paths: &[PathBuf],
        screen: (i32, i32),
    ) -> Result<(u64, u64), String> {
        self.validate_current(
            live,
            explorer,
            monitor_signature,
            viewport,
            live_visible_box_ids,
            received_paths,
        )?;
        let (group, tab) = self.target_at(live, screen)?;
        plan_native_ole_drop(
            live,
            explorer,
            &self.native_plan,
            &self.offered_paths,
            group,
            tab,
            self.max_groups,
            self.max_items_per_tab,
        )?;
        Ok((group, tab))
    }

    /// In-memory commit is atomic on the UI thread: all validations happen
    /// before assignment. Consumes the ticket to prevent duplicate commits.
    /// Persistence, its file lock and crash recovery MUST be implemented by
    /// a separate future production gate; this method never touches disk.
    pub fn commit(
        self,
        live: &mut State,
        explorer: &[ViewItem],
        monitor_signature: &str,
        viewport: NativeOleViewport,
        live_visible_box_ids: &[u64],
        received_paths: &[PathBuf],
        screen: (i32, i32),
    ) -> Result<NativeMultiBoxPlan, String> {
        let (group, tab) = self.preview(
            live,
            explorer,
            monitor_signature,
            viewport,
            live_visible_box_ids,
            received_paths,
            screen,
        )?;
        let (after, plan) = plan_native_ole_drop(
            live,
            explorer,
            &self.native_plan,
            &self.offered_paths,
            group,
            tab,
            self.max_groups,
            self.max_items_per_tab,
        )?;
        *live = after;
        Ok(plan)
    }
}

/// A future original-theme main-HWND OLE receiver can hold one of these per
/// drag gesture. All callbacks are intended for the owning UI thread. The
/// receiver must validate HWND ownership and live Macroquad visibility
/// independently; this contains no Windows handles and performs no Shell I/O.
#[derive(Default)]
pub struct NativeOleSession {
    pending: Option<NativeOleDropTicket>,
}

impl NativeOleSession {
    pub fn drag_enter(
        &mut self,
        state: &State,
        explorer: &[ViewItem],
        monitor_signature: &str,
        viewport: NativeOleViewport,
        paths: &[PathBuf],
        visible_box_ids: &[u64],
        max_groups: usize,
        max_items_per_tab: usize,
    ) -> Result<(), String> {
        // Re-entrant DragEnter must never retain a previous user's source.
        self.pending = None;
        self.pending = Some(NativeOleDropTicket::begin(
            state,
            explorer,
            monitor_signature,
            viewport,
            paths,
            visible_box_ids,
            max_groups,
            max_items_per_tab,
        )?);
        Ok(())
    }

    /// This is a candidate OLE effect, not a completed filesystem operation.
    /// Only a live, validated, visible target and source-offered LINK can be
    /// advertised. A pointer outside Boxes returns NONE; entering another
    /// valid Box later in the same gesture is still possible.
    pub fn drag_over_effect(
        &self,
        live: &State,
        explorer: &[ViewItem],
        monitor_signature: &str,
        viewport: NativeOleViewport,
        live_visible_box_ids: &[u64],
        offered_paths: &[PathBuf],
        screen: (i32, i32),
        effects_offered_by_source: u32,
    ) -> u32 {
        if effects_offered_by_source & 4 == 0 {
            return 0;
        }
        if self.pending.as_ref().is_some_and(|ticket| {
            ticket
                .preview(
                    live,
                    explorer,
                    monitor_signature,
                    viewport,
                    live_visible_box_ids,
                    offered_paths,
                    screen,
                )
                .is_ok()
        }) {
            4 // DROPEFFECT_LINK only
        } else {
            0
        }
    }

    pub fn drag_leave(&mut self) {
        self.pending = None;
    }

    /// Consume the gesture even on failure. A rejected/duplicate OLE Drop
    /// cannot replay the same source. Commit changes only the in-memory State.
    pub fn drop(
        &mut self,
        live: &mut State,
        explorer: &[ViewItem],
        monitor_signature: &str,
        viewport: NativeOleViewport,
        live_visible_box_ids: &[u64],
        received_paths: &[PathBuf],
        screen: (i32, i32),
        effects_offered_by_source: u32,
    ) -> Result<NativeMultiBoxPlan, String> {
        let ticket = self
            .pending
            .take()
            .ok_or("OLE Drop has no active validated gesture")?;
        if effects_offered_by_source & 4 == 0 {
            return Err("OLE Drop source has not offered a LINK effect".into());
        }
        ticket.commit(
            live,
            explorer,
            monitor_signature,
            viewport,
            live_visible_box_ids,
            received_paths,
            screen,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Bounds, Item};

    fn pin(path: &str) -> Item {
        Item {
            id: 1,
            path: path.into(),
            name: path.into(),
            kind: "lnk".into(),
            size: 0,
            modified: 0,
            label: None,
        }
    }
    fn fixture() -> (State, Vec<ViewItem>, NativeOleViewport, u64, u64) {
        let mut state = State::empty();
        let a = state.add_group(
            "Source",
            Bounds {
                x: 40.,
                y: 50.,
                w: 280.,
                h: 240.,
            },
        );
        state
            .group_mut(a)
            .unwrap()
            .tab_mut()
            .items
            .push(pin("C:/Desktop/A.lnk"));
        state.add_tab(a).unwrap();
        state
            .group_mut(a)
            .unwrap()
            .tab_mut()
            .items
            .push(pin("C:/Desktop/B.lnk"));
        state.group_mut(a).unwrap().active = 0;
        let b = state.add_group(
            "Target",
            Bounds {
                x: 350.,
                y: 50.,
                w: 280.,
                h: 240.,
            },
        );
        state
            .group_mut(b)
            .unwrap()
            .tab_mut()
            .items
            .push(pin("C:/Desktop/C.lnk"));
        state.add_tab(b).unwrap();
        state
            .group_mut(b)
            .unwrap()
            .tab_mut()
            .items
            .push(pin("C:/Desktop/D.lnk"));
        state.group_mut(b).unwrap().active = 0;
        let explorer = ["A", "B", "C", "D", "Loose"]
            .iter()
            .enumerate()
            .map(|(i, n)| ViewItem {
                path: format!("C:/Desktop/{n}.lnk"),
                label: format!("{n}"),
                pos: (i as i32 * 90, 20),
            })
            .collect();
        // Macroquad 800 x 400 UI, physically 1000 x 500 at 125% DPI,
        // with a negative virtual desktop origin.
        let view = NativeOleViewport {
            screen_origin_px: (-1400, 80),
            client_size_px: (1000, 500),
            logical_size: (800., 400.),
        };
        (state, explorer, view, a, b)
    }

    #[test]
    fn native_multi_selected_ole_pin_is_all_or_nothing_and_path_exact() {
        let (mut state,mut icons,view,source,target)=fixture();
        // Legacy imported shortcuts can have the same Item ID.
        // The native transaction must follow the verified Explorer paths.
        state.group_mut(source).unwrap().tabs[0].items.push(pin("C:/Desktop/E.lnk"));
        icons.push(ViewItem{
            path:"C:/Desktop/E.lnk".into(),label:"E".into(),pos:(500,20)
        });
        let selected=vec![PathBuf::from("C:/Desktop/A.lnk"),
                          PathBuf::from("C:/Desktop/E.lnk")];
        let visible=[source,target];
        let baseline=snapshot_state(&state).unwrap();
        let start=NativeOleDropTicket::begin(&state,&icons,
            "DISPLAY_A@125%",view,&selected,&visible,4,3).unwrap();
        assert!(start.preview(&state,&icons,"DISPLAY_A@125%",view,
            &visible,&selected,screen(430,160)).is_ok());
        let result=start.commit(&mut state,&icons,
            "DISPLAY_A@125%",view,&visible,&selected,screen(430,160)).unwrap();
        assert_eq!(state.group(source).unwrap().tabs[0].items.len(),0);
        let after=&state.group(target).unwrap().tabs[0].items;
        assert_eq!(after.len(),3);
        assert_eq!(after[0].path,"C:/Desktop/C.lnk");
        assert_eq!(after[1].path,"C:/Desktop/A.lnk");
        assert_eq!(after[2].path,"C:/Desktop/E.lnk");
        assert_eq!(result.groups.len(),2);
        assert_eq!(icons.len(),6); // Explorer items never changed

        let mut fresh=State::from_saved_bytes(&baseline).unwrap();
        for rejected in [
            vec![selected[0].clone(),selected[0].clone()],
            vec![selected[0].clone(),PathBuf::from("C:/Desktop/B.lnk")],
            vec![selected[0].clone(),PathBuf::from("C:/Desktop/C.lnk")],
            vec![selected[0].clone(),PathBuf::from("C:/Desktop/Loose.lnk")],
        ] {
            assert!(NativeOleDropTicket::begin(&fresh,&icons,
                "DISPLAY_A@125%",view,&rejected,&visible,4,3).is_err());
            assert_eq!(snapshot_state(&fresh).unwrap(),baseline);
        }
        let ticket=NativeOleDropTicket::begin(&fresh,&icons,
            "DISPLAY_A@125%",view,&selected,&visible,4,3).unwrap();
        assert!(ticket.commit(&mut fresh,&icons,"DISPLAY_A@125%",
            view,&visible,&selected,screen(430,160))
            .is_ok());
        assert_ne!(snapshot_state(&fresh).unwrap(),baseline);
    }

    #[test]
    fn multi_selection_stale_or_over_capacity_does_not_change_state() {
        let (mut state,mut icons,view,source,target)=fixture();
        state.group_mut(source).unwrap().tabs[0].items.push(pin("C:/Desktop/E.lnk"));
        icons.push(ViewItem{
            path:"C:/Desktop/E.lnk".into(),label:"E".into(),pos:(500,20)
        });
        let selected=vec![PathBuf::from("C:/Desktop/A.lnk"),
                          PathBuf::from("C:/Desktop/E.lnk")];
        let visible=[source,target];
        let baseline=snapshot_state(&state).unwrap();
        // Destination has 1 item, two transfers would exceed 2 per tab.
        let capacity_ticket=NativeOleDropTicket::begin(
            &state,&icons,"DISPLAY_A@125%",view,&selected,&visible,4,2).unwrap();
        assert!(capacity_ticket.commit(&mut state,&icons,"DISPLAY_A@125%",
            view,&visible,&selected,screen(430,160)).is_err());
        assert_eq!(snapshot_state(&state).unwrap(),baseline);

        let stale_ticket=NativeOleDropTicket::begin(
            &state,&icons,"DISPLAY_A@125%",view,&selected,&visible,4,3).unwrap();
        let mut replaced=selected.clone();
        replaced.reverse();
        assert!(stale_ticket.commit(&mut state,&icons,"DISPLAY_A@125%",
            view,&visible,&replaced,screen(430,160)).is_err());
        assert_eq!(snapshot_state(&state).unwrap(),baseline);

        let geometry_ticket=NativeOleDropTicket::begin(
            &state,&icons,"DISPLAY_A@125%",view,&selected,&visible,4,3).unwrap();
        icons[0].pos=(1245,24);
        assert!(geometry_ticket.commit(&mut state,&icons,"DISPLAY_A@125%",
            view,&visible,&selected,screen(430,160)).is_err());
        assert_eq!(snapshot_state(&state).unwrap(),baseline);
    }
    fn screen(x: i32, y: i32) -> (i32, i32) {
        (
            -1400 + (x as f64 * 1.25) as i32,
            80 + (y as f64 * 1.25) as i32,
        )
    }
    fn visible_ids(state: &State) -> Vec<u64> {
        state.groups.iter().map(|g| g.id).collect()
    }
    fn offered() -> Vec<PathBuf> {
        vec![PathBuf::from("C:/Desktop/A.lnk")]
    }
    fn ticket(state: &State, items: &[ViewItem], v: NativeOleViewport) -> NativeOleDropTicket {
        NativeOleDropTicket::begin(
            state,
            items,
            "DISPLAY_A@125%",
            v,
            &[PathBuf::from("C:/Desktop/A.lnk")],
            &visible_ids(state),
            4,
            3,
        )
        .unwrap()
    }

    #[test]
    fn physical_geometry_supports_negative_monitor_origin_and_dpi() {
        let (_, _, v, _, _) = fixture();
        let logical = v.logical_at(screen(420, 150)).unwrap();
        assert!((logical.0 - 420.).abs() <= 1.);
        assert!((logical.1 - 150.).abs() <= 1.);
        assert!(v.logical_at((-1500, 220)).is_err());
        assert!(v.logical_at((-400, 220)).is_err()); // exclusive far edge
        assert!(NativeOleViewport {
            client_size_px: (0, 500),
            ..v
        }
        .logical_at(screen(400, 150))
        .is_err());
        assert!(NativeOleViewport {
            logical_size: (f64::NAN, 400.),
            ..v
        }
        .logical_at(screen(400, 150))
        .is_err());
        assert!(NativeOleViewport {
            logical_size: (150., 400.),
            ..v
        }
        .logical_at(screen(400, 150))
        .is_err()); // unreasonable nonuniform DPI
    }
    #[test]
    fn only_the_body_of_a_distinct_visible_box_accepts_a_drop() {
        let (state, icons, v, source, target) = fixture();
        let t = ticket(&state, &icons, v);
        assert_eq!(
            t.preview(
                &state,
                &icons,
                "DISPLAY_A@125%",
                v,
                &visible_ids(&state),
                &offered(),
                screen(420, 150)
            )
            .unwrap()
            .0,
            target
        );
        assert!(t
            .preview(
                &state,
                &icons,
                "DISPLAY_A@125%",
                v,
                &visible_ids(&state),
                &offered(),
                screen(420, 65)
            )
            .is_err()); // titlebar
        assert!(t
            .preview(
                &state,
                &icons,
                "DISPLAY_A@125%",
                v,
                &visible_ids(&state),
                &offered(),
                screen(420, 52)
            )
            .is_err());
        assert!(t
            .preview(
                &state,
                &icons,
                "DISPLAY_A@125%",
                v,
                &visible_ids(&state),
                &offered(),
                screen(60, 150)
            )
            .is_err()); // source
        assert!(t
            .preview(
                &state,
                &icons,
                "DISPLAY_A@125%",
                v,
                &visible_ids(&state),
                &offered(),
                screen(710, 150)
            )
            .is_err());
        assert!(t
            .preview(
                &state,
                &icons,
                "DISPLAY_A@125%",
                v,
                &visible_ids(&state),
                &offered(),
                screen(354, 100)
            )
            .is_err()); // resize
        assert_eq!(source, state.groups[0].id);
    }
    #[test]
    fn in_memory_commit_is_single_use_and_no_mutation_on_failed_gates() {
        let (mut state, icons, v, source, target) = fixture();
        let before = snapshot_state(&state).unwrap();
        let stale = ticket(&state, &icons, v);
        let mut changed = state.clone();
        changed.group_mut(target).unwrap().bounds.x = 410.;
        assert!(stale
            .commit(
                &mut changed,
                &icons,
                "DISPLAY_A@125%",
                v,
                &visible_ids(&state),
                &offered(),
                screen(420, 150)
            )
            .is_err());
        assert_eq!(changed.group(target).unwrap().bounds.x, 410.);
        assert_eq!(snapshot_state(&state).unwrap(), before);
        let t = ticket(&state, &icons, v);
        let live_visible = visible_ids(&state);
        let plan = t
            .commit(
                &mut state,
                &icons,
                "DISPLAY_A@125%",
                v,
                &live_visible,
                &offered(),
                screen(420, 150),
            )
            .unwrap();
        assert!(state.group(source).unwrap().tabs[0].items.is_empty());
        assert_eq!(state.group(target).unwrap().tabs[0].items.len(), 2);
        assert_eq!(plan.hidden_for(&[]).unwrap(), vec![1, 3]);
    }
    #[test]
    fn stale_explorer_identity_position_page_or_monitor_fails_closed() {
        let (state, icons, v, _, target) = fixture();
        let mut moved = icons.clone();
        moved[0].pos = (999, 999);
        assert!(ticket(&state, &icons, v)
            .preview(
                &state,
                &moved,
                "DISPLAY_A@125%",
                v,
                &visible_ids(&state),
                &offered(),
                screen(420, 150)
            )
            .is_err());
        let mut reordered = icons.clone();
        reordered.swap(1, 2);
        assert!(ticket(&state, &icons, v)
            .preview(
                &state,
                &reordered,
                "DISPLAY_A@125%",
                v,
                &visible_ids(&state),
                &offered(),
                screen(420, 150)
            )
            .is_err());
        let mut relabel = icons.clone();
        relabel[0].label = "New Label".into();
        assert!(ticket(&state, &icons, v)
            .preview(
                &state,
                &relabel,
                "DISPLAY_A@125%",
                v,
                &visible_ids(&state),
                &offered(),
                screen(420, 150)
            )
            .is_err());
        assert!(ticket(&state, &icons, v)
            .preview(
                &state,
                &icons,
                "DISPLAY_B@125%",
                v,
                &visible_ids(&state),
                &offered(),
                screen(420, 150)
            )
            .is_err());
        assert!(ticket(&state, &icons, v)
            .preview(
                &state,
                &icons,
                "DISPLAY_A@125%",
                NativeOleViewport {
                    screen_origin_px: (-1300, 80),
                    ..v
                },
                &visible_ids(&state),
                &offered(),
                screen(420, 150)
            )
            .is_err());
        let mut page = state.clone();
        page.group_mut(target).unwrap().rolled = true;
        assert!(ticket(&state, &icons, v)
            .preview(
                &page,
                &icons,
                "DISPLAY_A@125%",
                v,
                &visible_ids(&state),
                &offered(),
                screen(420, 150)
            )
            .is_err());
    }
    #[test]
    fn overlapping_box_portal_and_hidden_tab_fail_closed() {
        let (state, icons, v, _, target) = fixture();
        let mut overlapping = state.clone();
        overlapping.group_mut(target).unwrap().bounds.x = 45.;
        assert!(ticket(&state, &icons, v)
            .preview(
                &overlapping,
                &icons,
                "DISPLAY_A@125%",
                v,
                &visible_ids(&state),
                &offered(),
                screen(420, 150)
            )
            .is_err());
        let mut portal = state.clone();
        portal.group_mut(target).unwrap().tabs[0].folder = Some("D:/".into());
        assert!(NativeOleDropTicket::begin(
            &portal,
            &icons,
            "DISPLAY_A@125%",
            v,
            &[PathBuf::from("C:/Desktop/A.lnk")],
            &visible_ids(&portal),
            4,
            3
        )
        .is_err());
        assert!(NativeOleDropTicket::begin(
            &state,
            &icons,
            "",
            v,
            &[PathBuf::from("C:/Desktop/A.lnk")],
            &visible_ids(&state),
            4,
            3
        )
        .is_err());
        assert!(NativeOleDropTicket::begin(
            &state,
            &icons,
            "DISPLAY_A@125%",
            v,
            &[PathBuf::from("C:/Desktop/Loose.lnk")],
            &visible_ids(&state),
            4,
            3
        )
        .is_err());
        assert!(NativeOleDropTicket::begin(
            &state,
            &icons,
            "DISPLAY_A@125%",
            v,
            &[
                PathBuf::from("C:/Desktop/A.lnk"),
                PathBuf::from("C:/Desktop/B.lnk")
            ],
            &visible_ids(&state),
            4,
            3
        )
        .is_err());
    }

    #[test]
    fn visible_box_allowlist_cannot_change_during_drag() {
        let (mut state, icons, v, source, target) = fixture();
        let mut ticket = ticket(&state, &icons, v);
        let requested = screen(420, 150);
        assert_eq!(
            ticket
                .preview(
                    &state,
                    &icons,
                    "DISPLAY_A@125%",
                    v,
                    &[source, target],
                    &offered(),
                    requested
                )
                .unwrap()
                .0,
            target
        );
        let substitute = [PathBuf::from("C:/Desktop/C.lnk")];
        assert!(ticket
            .preview(
                &state,
                &icons,
                "DISPLAY_A@125%",
                v,
                &[source, target],
                &substitute,
                requested
            )
            .is_err());
        assert!(ticket
            .preview(
                &state,
                &icons,
                "DISPLAY_A@125%",
                v,
                &[source, target],
                &[],
                requested
            )
            .is_err());
        assert!(ticket
            .preview(
                &state,
                &icons,
                "DISPLAY_A@125%",
                v,
                &[source],
                &offered(),
                requested
            )
            .is_err()); // destination hidden
        assert!(ticket
            .preview(
                &state,
                &icons,
                "DISPLAY_A@125%",
                v,
                &[target, source, source],
                &offered(),
                requested
            )
            .is_err()); // duplicate visibility identity
        assert!(ticket
            .preview(
                &state,
                &icons,
                "DISPLAY_A@125%",
                v,
                &[source, target, 123456],
                &offered(),
                requested
            )
            .is_err());
        let initial = snapshot_state(&state).unwrap();
        assert!(ticket
            .commit(
                &mut state,
                &icons,
                "DISPLAY_A@125%",
                v,
                &[source],
                &offered(),
                requested
            )
            .is_err());
        assert_eq!(snapshot_state(&state).unwrap(), initial); // all-or-nothing
        ticket = NativeOleDropTicket::begin(
            &state,
            &icons,
            "DISPLAY_A@125%",
            v,
            &[PathBuf::from("C:/Desktop/A.lnk")],
            &[source, target],
            4,
            3,
        )
        .unwrap();
        ticket.created_at = Instant::now() - Duration::from_secs(91);
        assert!(ticket
            .preview(
                &state,
                &icons,
                "DISPLAY_A@125%",
                v,
                &[source, target],
                &offered(),
                requested
            )
            .is_err()); // stale gesture lease
    }

    #[test]
    fn genuine_overlapping_destination_boxes_and_replayed_ticket_fail_closed() {
        let (mut state, mut icons, v, source, target) = fixture();
        let extra = state.add_group("Overlap", state.group(target).unwrap().bounds);
        state
            .group_mut(extra)
            .unwrap()
            .tab_mut()
            .items
            .push(pin("C:/Desktop/E.lnk"));
        state.add_tab(extra).unwrap();
        state
            .group_mut(extra)
            .unwrap()
            .tab_mut()
            .items
            .push(pin("C:/Desktop/F.lnk"));
        state.group_mut(extra).unwrap().active = 0;
        icons.push(ViewItem {
            path: "C:/Desktop/E.lnk".into(),
            label: "E".into(),
            pos: (600, 20),
        });
        icons.push(ViewItem {
            path: "C:/Desktop/F.lnk".into(),
            label: "F".into(),
            pos: (700, 20),
        });
        let listed = [source, target, extra];
        let proposed = NativeOleDropTicket::begin(
            &state,
            &icons,
            "DISPLAY_A@125%",
            v,
            &[PathBuf::from("C:/Desktop/A.lnk")],
            &listed,
            4,
            3,
        )
        .unwrap();
        assert!(proposed
            .preview(
                &state,
                &icons,
                "DISPLAY_A@125%",
                v,
                &listed,
                &offered(),
                screen(420, 150)
            )
            .is_err());
        let original = snapshot_state(&state).unwrap();
        assert!(proposed
            .commit(
                &mut state,
                &icons,
                "DISPLAY_A@125%",
                v,
                &listed,
                &offered(),
                screen(420, 150)
            )
            .is_err());
        assert_eq!(snapshot_state(&state).unwrap(), original);

        // Resolve the overlap in the initial frame, then prove a second
        // ticket captured before the first commit cannot replay the move.
        state.group_mut(extra).unwrap().bounds.x = 650.;
        let first = NativeOleDropTicket::begin(
            &state,
            &icons,
            "DISPLAY_A@125%",
            v,
            &[PathBuf::from("C:/Desktop/A.lnk")],
            &listed,
            4,
            3,
        )
        .unwrap();
        let stale = NativeOleDropTicket::begin(
            &state,
            &icons,
            "DISPLAY_A@125%",
            v,
            &[PathBuf::from("C:/Desktop/A.lnk")],
            &listed,
            4,
            3,
        )
        .unwrap();
        assert!(first
            .commit(
                &mut state,
                &icons,
                "DISPLAY_A@125%",
                v,
                &listed,
                &offered(),
                screen(420, 150)
            )
            .is_ok());
        let committed = snapshot_state(&state).unwrap();
        assert!(stale
            .commit(
                &mut state,
                &icons,
                "DISPLAY_A@125%",
                v,
                &listed,
                &offered(),
                screen(420, 150)
            )
            .is_err());
        assert_eq!(snapshot_state(&state).unwrap(), committed);
    }

    #[test]
    fn accepted_virtual_pin_transfer_does_not_write_user_workspace_or_disk() {
        let (mut state, icons, v, source, target) = fixture();
        let root = std::env::temp_dir().join(format!(
            "DesktopCraft-OLE-NoDisk-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let persisted = root.join("workspace.json");
        state.save(&persisted).unwrap();
        let saved_bytes = std::fs::read(&persisted).unwrap();
        let ticket = NativeOleDropTicket::begin(
            &state,
            &icons,
            "DISPLAY_A@125%",
            v,
            &offered(),
            &[source, target],
            4,
            3,
        )
        .unwrap();
        let successful = ticket.commit(
            &mut state,
            &icons,
            "DISPLAY_A@125%",
            v,
            &[source, target],
            &offered(),
            screen(420, 150),
        );
        assert!(successful.is_ok());
        assert!(state.group(source).unwrap().tabs[0].items.is_empty());
        assert_eq!(state.group(target).unwrap().tabs[0].items.len(), 2);
        assert_eq!(std::fs::read(&persisted).unwrap(), saved_bytes);
        let children = std::fs::read_dir(&root)
            .unwrap()
            .map(|r| r.unwrap().file_name().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(children, vec!["workspace.json"]); // no backup/temp side effects
        std::fs::remove_file(persisted).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn virtual_ole_session_handles_hover_leave_and_one_time_drop_without_win32() {
        let (mut state, icons, view, source, target) = fixture();
        let exposed = [source, target];
        let payload = offered();
        let mut session = NativeOleSession::default();
        let good_point = screen(430, 160);
        let bad_point = screen(700, 160);

        assert!(session
            .drop(
                &mut state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &exposed,
                &payload,
                good_point,
                7
            )
            .is_err()); // no DragEnter
        session
            .drag_enter(
                &state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &payload,
                &exposed,
                4,
                3,
            )
            .unwrap();
        assert_eq!(
            session.drag_over_effect(
                &state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &exposed,
                &payload,
                good_point,
                7
            ),
            4
        );
        assert_eq!(
            session.drag_over_effect(
                &state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &exposed,
                &payload,
                good_point,
                2
            ),
            0
        ); // source allows MOVE only
        assert_eq!(
            session.drag_over_effect(
                &state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &exposed,
                &payload,
                bad_point,
                7
            ),
            0
        );
        assert_eq!(
            session.drag_over_effect(
                &state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &exposed,
                &payload,
                good_point,
                7
            ),
            4
        ); // return to valid Box
        session.drag_leave();
        assert_eq!(
            session.drag_over_effect(
                &state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &exposed,
                &payload,
                good_point,
                7
            ),
            0
        );
        assert!(session
            .drop(
                &mut state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &exposed,
                &payload,
                good_point,
                7
            )
            .is_err());

        session
            .drag_enter(
                &state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &payload,
                &exposed,
                4,
                3,
            )
            .unwrap();
        let unchanged = snapshot_state(&state).unwrap();
        assert!(session
            .drop(
                &mut state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &exposed,
                &payload,
                good_point,
                2
            )
            .is_err());
        assert_eq!(snapshot_state(&state).unwrap(), unchanged);

        session
            .drag_enter(
                &state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &payload,
                &exposed,
                4,
                3,
            )
            .unwrap();
        assert!(session
            .drop(
                &mut state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &exposed,
                &payload,
                good_point,
                7
            )
            .is_ok());
        assert!(session
            .drop(
                &mut state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &exposed,
                &payload,
                good_point,
                7
            )
            .is_err()); // ticket consumed
        assert!(state.group(source).unwrap().tabs[0].items.is_empty());
        assert_eq!(state.group(target).unwrap().tabs[0].items.len(), 2);
        assert!(session
            .drag_enter(
                &state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &payload,
                &exposed,
                4,
                3
            )
            .is_ok()); // new gesture may legitimately move it back
        assert_eq!(
            session.drag_over_effect(
                &state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &exposed,
                &payload,
                good_point,
                7
            ),
            0 // it is now in the destination Box; same-Box drops are rejected
        );
        assert_eq!(
            session.drag_over_effect(
                &state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &exposed,
                &payload,
                screen(100, 160),
                7
            ),
            4 // a new distinct source->destination transfer is allowed
        );
        session.drag_leave();
    }

    #[test]
    fn virtual_ole_session_rejects_source_replacement_even_when_link_was_offered() {
        let (mut state, icons, view, source, target) = fixture();
        let live = [source, target];
        let mut session = NativeOleSession::default();
        session
            .drag_enter(
                &state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &offered(),
                &live,
                4,
                3,
            )
            .unwrap();
        let changed = [PathBuf::from("C:/Desktop/B.lnk")];
        let at = screen(430, 160);
        assert_eq!(
            session.drag_over_effect(
                &state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &live,
                &changed,
                at,
                7
            ),
            0
        );
        let original = snapshot_state(&state).unwrap();
        assert!(session
            .drop(
                &mut state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &live,
                &changed,
                at,
                7
            )
            .is_err());
        assert_eq!(snapshot_state(&state).unwrap(), original);
        assert_eq!(
            session.drag_over_effect(
                &state,
                &icons,
                "DISPLAY_A@125%",
                view,
                &live,
                &offered(),
                at,
                7
            ),
            0
        ); // no pending gesture remains
    }
}
