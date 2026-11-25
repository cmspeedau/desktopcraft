use super::*;

/// Time Explorer needs to re-snap icons placed on a monitor whose scale
/// differs from the main monitor's (observed under ~1 s on 2026-11-19).
pub(super) const SCALED_SETTLE_MS: u64 = 1200;

/// Explorer may settle a scaled-monitor icon a pixel or two from the
/// computed cell (DPI rounding). Anything further is a different cell.
pub(super) fn within_settle_tolerance(now:(i32,i32),dest:(i32,i32))->bool{
    (now.0-dest.0).abs()<=3 && (now.1-dest.1).abs()<=3
}

#[cfg(test)]
#[test]
pub(super) fn settle_tolerance_accepts_rounding_not_a_neighbour_cell(){
    assert!(within_settle_tolerance((2856,269),(2857,270)));
    assert!(within_settle_tolerance((2856,269),(2853,272)));
    assert!(!within_settle_tolerance((2856,269),(2860,269)));
    assert!(!within_settle_tolerance((2856,403),(2856,269)));
}

/// Orders one layout generation so Explorer only moves icons whose cell
/// changes, and never onto a cell another moving icon still occupies.
/// Icons leaving every Box return to their original cell first. A cycle
/// among moving icons is broken once by parking them on their originals.
pub(super) fn plan_incremental_moves(
    old:&[Move], next:&[Move], current:&[ViewItem], cell:(i32,i32),
    original:impl Fn(&str)->Option<(i32,i32)>,
)->Result<Vec<Vec<Move>>,String>{
    let mut now:BTreeMap<String,(i32,i32)>=current.iter()
        .map(|icon|(icon.path.to_lowercase(),icon.pos)).collect();
    let original_of=|path:&str|original(path)
        .ok_or_else(||"Native original icon checkpoint incomplete".to_string());
    let mut steps=Vec::new();
    let mut leaving=Vec::new();
    for (path,_) in old {
        if next.iter().any(|(placed,_)|placed.eq_ignore_ascii_case(path)) {continue;}
        let to=original_of(path)?;
        if now.get(&path.to_lowercase())!=Some(&to) {leaving.push((path.clone(),to));}
    }
    if !leaving.is_empty() {
        for (path,to) in &leaving {now.insert(path.to_lowercase(),*to);}
        steps.push(leaving);
    }
    if next.iter().any(|(path,_)|!now.contains_key(&path.to_lowercase())) {
        return Err("Native Box icon missing from Explorer".into());
    }
    let mut pending:Vec<Move>=next.iter()
        .filter(|(path,to)|now.get(&path.to_lowercase())!=Some(to)).cloned().collect();
    let mut parked=false;
    while !pending.is_empty() {
        let (ready,rest):(Vec<Move>,Vec<Move>)=pending.iter().cloned().partition(|(path,to)|
            !pending.iter().any(|(other,_)|!other.eq_ignore_ascii_case(path)
                && native_box_placement::cells_overlap(now[&other.to_lowercase()],*to,cell)));
        if ready.is_empty() {
            if parked {return Err("Native Box icon moves form an unresolved cell cycle".into());}
            parked=true;
            let park=rest.iter().map(|(path,_)|Ok((path.clone(),original_of(path)?)))
                .collect::<Result<Vec<Move>,String>>()?;
            for (path,to) in &park {now.insert(path.to_lowercase(),*to);}
            steps.push(park);
            continue;
        }
        for (path,to) in &ready {now.insert(path.to_lowercase(),*to);}
        steps.push(ready);
        pending=rest;
    }
    Ok(steps)
}

#[cfg(test)]
mod incremental_move_tests {
    use super::*;
    fn icon(path:&str,pos:(i32,i32))->ViewItem{
        ViewItem{path:path.into(),label:path.into(),pos}
    }
    fn mv(path:&str,pos:(i32,i32))->Move{(path.into(),pos)}
    const CELL:(i32,i32)=(90,100);
    fn originals(path:&str)->Option<(i32,i32)>{
        match path {"a"=>Some((0,1000)),"b"=>Some((90,1000)),"c"=>Some((180,1000)),_=>None}
    }

    #[test]
    fn unchanged_box_icons_are_not_moved_at_all(){
        let now=[icon("a",(500,100)),icon("b",(590,100))];
        let same=[mv("a",(500,100)),mv("b",(590,100))];
        assert!(plan_incremental_moves(&same,&same,&now,CELL,originals).unwrap().is_empty());
    }

    #[test]
    fn tab_switch_sends_old_icons_home_then_brings_new_ones_in(){
        let now=[icon("a",(500,100)),icon("b",(90,1000))];
        let steps=plan_incremental_moves(&[mv("a",(500,100))],&[mv("b",(500,100))],
            &now,CELL,originals).unwrap();
        assert_eq!(steps,vec![vec![mv("a",(0,1000))],vec![mv("b",(500,100))]]);
    }

    #[test]
    fn chained_shift_moves_the_icon_whose_target_is_free_first(){
        // a moves into b's cell, b moves to a free cell: b must go first.
        let now=[icon("a",(500,100)),icon("b",(590,100))];
        let steps=plan_incremental_moves(&[mv("a",(500,100)),mv("b",(590,100))],
            &[mv("a",(590,100)),mv("b",(680,100))],&now,CELL,originals).unwrap();
        assert_eq!(steps,vec![vec![mv("b",(680,100))],vec![mv("a",(590,100))]]);
    }

    #[test]
    fn swapped_icons_are_parked_on_their_originals_once(){
        let now=[icon("a",(500,100)),icon("b",(590,100))];
        let steps=plan_incremental_moves(&[mv("a",(500,100)),mv("b",(590,100))],
            &[mv("a",(590,100)),mv("b",(500,100))],&now,CELL,originals).unwrap();
        assert_eq!(steps,vec![
            vec![mv("a",(0,1000)),mv("b",(90,1000))],
            vec![mv("a",(590,100)),mv("b",(500,100))],
        ]);
    }

    #[test]
    fn missing_original_or_icon_refuses_before_any_move(){
        let now=[icon("a",(500,100))];
        assert!(plan_incremental_moves(&[mv("zz",(500,100))],&[],&now,CELL,originals).is_err());
        assert!(plan_incremental_moves(&[],&[mv("c",(500,100))],&now,CELL,originals).is_err());
    }
}

/// Separate collateral Explorer unstacking of hidden parked icons from real
/// visible icon movement. Only icons explicitly tracked in `parked_paths`,
/// expected at `hidden_park`, unchanged in label, and (when `eligible` is
/// provided) currently hidden (`STATE_HIDE`) qualify.
pub(crate) fn split_displaced_hidden_park(
    expected: &[ViewItem],
    observed: &[ViewItem],
    parked_paths: &BTreeSet<String>,
    hidden_park: Option<(i32, i32)>,
    eligible: Option<&std::collections::HashSet<String>>,
) -> (Vec<String>, Vec<ViewItem>) {
    let Some(park) = hidden_park else {
        return (Vec::new(), observed.to_vec());
    };
    if parked_paths.is_empty() {
        return (Vec::new(), observed.to_vec());
    }
    let expected_by_key = expected
        .iter()
        .map(|item| (item.path.to_lowercase(), item))
        .collect::<BTreeMap<_, _>>();
    let mut displaced = Vec::new();
    let mut filtered = Vec::with_capacity(observed.len());
    for item in observed {
        let key = item.path.to_lowercase();
        let is_displaced_park = parked_paths.contains(&key)
            && eligible.is_none_or(|e| !e.contains(&key))
            && expected_by_key
                .get(&key)
                .is_some_and(|exp| exp.pos == park && exp.label == item.label)
            && item.pos != park;
        if is_displaced_park {
            displaced.push(item.path.clone());
            filtered.push(ViewItem { pos: park, ..item.clone() });
        } else {
            filtered.push(item.clone());
        }
    }
    (displaced, filtered)
}

pub(super) fn same_icon_positions(a:&[ViewItem],b:&[ViewItem])->bool{
    let index = |items: &[ViewItem]| items.iter().map(|item|
        (item.path.to_lowercase(), (item.label.clone(), item.pos)))
        .collect::<BTreeMap<_, _>>();
    let left = index(a);
    let right = index(b);
    left.len() == a.len() && right.len() == b.len() && left == right
}

pub(super) fn same_move_set(a: &[Move], b: &[Move]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let map_a = a.iter()
        .map(|(path, pos)| (path.to_lowercase(), *pos))
        .collect::<BTreeMap<_, _>>();
    let map_b = b.iter()
        .map(|(path, pos)| (path.to_lowercase(), *pos))
        .collect::<BTreeMap<_, _>>();
    map_a.len() == a.len() && map_b.len() == b.len() && map_a == map_b
}

pub(super) fn can_attempt_fast_native_batch(
    native_batch_shell_verified: bool,
    snap_to_grid: bool,
    has_pending_scaled_recheck: bool,
    has_scaled_targets: bool,
    has_steps: bool,
    nudged_unchanged: bool,
) -> bool {
    native_batch_shell_verified
        && !snap_to_grid
        && !has_pending_scaled_recheck
        && !has_scaled_targets
        && has_steps
        && nudged_unchanged
}

/// Move planner when Explorer's global Align-to-Grid is disabled: hidden icons
/// may stack at one free on-screen parking point without bouncing, and visible
/// Wait adaptively for Explorer ListView positions to settle instead of
/// sleeping a fixed worst-case interval on every reflow.
pub(super) fn wait_until_settled(
    steps_empty: bool,
    delays_ms: &[u64],
    mut check: impl FnMut() -> (bool, Vec<ViewItem>),
) -> Vec<ViewItem> {
    if steps_empty || delays_ms.is_empty() {
        return check().1;
    }
    // A zero first delay probes immediately. Count the probe time toward the
    // first retry so reading an unfinished layout does not add a full wait.
    let mut first_probe_elapsed = Duration::ZERO;
    let delays_ms = if let Some((&0, retries)) = delays_ms.split_first() {
        let start = std::time::Instant::now();
        let (ready, items) = check();
        if ready || retries.is_empty() {
            return items;
        }
        first_probe_elapsed = start.elapsed();
        retries
    } else {
        delays_ms
    };
    for (idx, &ms) in delays_ms.iter().enumerate() {
        let delay = Duration::from_millis(ms).saturating_sub(first_probe_elapsed);
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
        first_probe_elapsed = Duration::ZERO;
        let (ready, items) = check();
        if ready || idx + 1 == delays_ms.len() {
            return items;
        }
    }
    unreachable!()
}

pub(super) fn positions_settled(
    current: &[ViewItem],
    original: &[ViewItem],
    placement: &[Move],
    nudged: &[Move],
    scaled_paths: &BTreeSet<String>,
) -> bool {
    if current.len() != original.len() {
        return false;
    }
    let now_by_path = current.iter()
        .map(|now| (now.path.to_lowercase(), now.pos))
        .collect::<BTreeMap<_, _>>();
    if now_by_path.len() != current.len() {
        return false;
    }
    let placed_paths = placement.iter().chain(nudged.iter())
        .map(|(p, _)| p.to_lowercase())
        .collect::<BTreeSet<_>>();
    let landed = |(path, dest): &(String, (i32, i32))| {
        let key = path.to_lowercase();
        let Some(&pos) = now_by_path.get(&key) else { return false; };
        pos == *dest || (scaled_paths.contains(&key) && within_settle_tolerance(pos, *dest))
    };
    if !placement.iter().all(landed) || !nudged.iter().all(landed) {
        return false;
    }
    if original.iter().filter(|icon| !placed_paths.contains(&icon.path.to_lowercase()))
        .any(|icon| now_by_path.get(&icon.path.to_lowercase()) != Some(&icon.pos))
    {
        return false;
    }
    let scaled_cells = current.iter()
        .filter(|now| scaled_paths.contains(&now.path.to_lowercase()))
        .map(|now| now.pos)
        .collect::<Vec<_>>();
    !scaled_cells.iter().enumerate().any(|(n, a)| scaled_cells[n + 1..].contains(a))
}

/// Break rollback overlaps/cycles cleanly so destinations occupied by another
/// moving icon are vacated first when restoring Explorer positions.
pub(super) fn order_rollback_batches(current: &[ViewItem], rollback: &[Move]) -> Vec<Vec<Move>> {
    if rollback.len() <= 1 {
        return if rollback.is_empty() { Vec::new() } else { vec![rollback.to_vec()] };
    }
    let mut now = current.iter()
        .map(|item| (item.path.to_lowercase(), item.pos))
        .collect::<BTreeMap<_, _>>();
    let mut pending = rollback.iter()
        .filter(|(path, dest)| now.get(&path.to_lowercase()) != Some(dest))
        .cloned()
        .collect::<Vec<_>>();
    let mut batches = Vec::new();
    while !pending.is_empty() {
        let moving_pos = pending.iter()
            .filter_map(|(path, _)| now.get(&path.to_lowercase()).copied().map(|p| (path.to_lowercase(), p)))
            .collect::<Vec<_>>();
        let (ready, rest): (Vec<Move>, Vec<Move>) = pending.into_iter().partition(|(path, dest)| {
            let key = path.to_lowercase();
            !moving_pos.iter().any(|(other, pos)| other != &key && pos == dest)
        });
        if ready.is_empty() {
            batches.push(rest);
            break;
        }
        for (path, dest) in &ready {
            now.insert(path.to_lowercase(), *dest);
        }
        batches.push(ready);
        pending = rest;
    }
    batches
}

/// Box icons land directly at their Box-grid cells. Batch 1 clears leaving,
/// newly parked hidden icons, and nudged loose icons; Batch 2 places changed
/// visible Box icons.
pub(super) fn plan_free_moves(
    old:&[Move], old_nudged:&[Move], visible:&[Move], parked:&[Move], nudged:&[Move],
    current:&[ViewItem], original:impl Fn(&str)->Option<(i32,i32)>,
)->Result<Vec<Vec<Move>>,String>{
    let now:BTreeMap<String,(i32,i32)>=current.iter()
        .map(|icon|(icon.path.to_lowercase(),icon.pos)).collect();
    let next_paths:BTreeSet<String>=visible.iter().chain(parked.iter()).chain(nudged.iter())
        .map(|(path,_)|path.to_lowercase()).collect();
    let mut batch1=Vec::new();
    let mut seen_return=BTreeSet::new();
    for (path,_) in old.iter().chain(old_nudged.iter()) {
        let key=path.to_lowercase();
        if next_paths.contains(&key) || !seen_return.insert(key.clone()) {continue;}
        let to=original(path).ok_or_else(||"Native original icon checkpoint incomplete".to_string())?;
        if now.get(&key)!=Some(&to) {batch1.push((path.clone(),to));}
    }
    for (path,to) in parked.iter().chain(nudged.iter()) {
        let key=path.to_lowercase();
        if !now.contains_key(&key) {return Err("Native Box icon missing from Explorer".into());}
        if now.get(&key)!=Some(to) {batch1.push((path.clone(),*to));}
    }
    let mut batch2=Vec::new();
    for (path,to) in visible {
        let key=path.to_lowercase();
        if !now.contains_key(&key) {return Err("Native Box icon missing from Explorer".into());}
        if now.get(&key)!=Some(to) {batch2.push((path.clone(),*to));}
    }
    let mut steps=Vec::new();
    if !batch1.is_empty() {steps.push(batch1);}
    if !batch2.is_empty() {steps.push(batch2);}
    Ok(steps)
}

