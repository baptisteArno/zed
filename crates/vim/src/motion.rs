use std::sync::Arc;

use editor::{
    char_kind,
    display_map::{DisplaySnapshot, ToDisplayPoint},
    movement, Bias, CharKind, DisplayPoint, ToOffset,
};
use gpui::{actions, impl_actions, AppContext, WindowContext};
use language::{Point, Selection, SelectionGoal};
use serde::Deserialize;
use workspace::Workspace;

use crate::{
    normal::normal_motion,
    state::{Mode, Operator},
    visual::visual_motion,
    Vim,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Motion {
    Left,
    Backspace,
    Down,
    Up,
    Right,
    NextWordStart { ignore_punctuation: bool },
    NextWordEnd { ignore_punctuation: bool },
    PreviousWordStart { ignore_punctuation: bool },
    FirstNonWhitespace,
    CurrentLine,
    StartOfLine,
    EndOfLine,
    StartOfParagraph,
    EndOfParagraph,
    StartOfDocument,
    EndOfDocument,
    Matching,
    FindForward { before: bool, text: Arc<str> },
    FindBackward { after: bool, text: Arc<str> },
    NextLineStart,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct NextWordStart {
    #[serde(default)]
    ignore_punctuation: bool,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct NextWordEnd {
    #[serde(default)]
    ignore_punctuation: bool,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct PreviousWordStart {
    #[serde(default)]
    ignore_punctuation: bool,
}

#[derive(Clone, Deserialize, PartialEq)]
struct RepeatFind {
    #[serde(default)]
    backwards: bool,
}

actions!(
    vim,
    [
        Left,
        Backspace,
        Down,
        Up,
        Right,
        FirstNonWhitespace,
        StartOfLine,
        EndOfLine,
        CurrentLine,
        StartOfParagraph,
        EndOfParagraph,
        StartOfDocument,
        EndOfDocument,
        Matching,
        NextLineStart,
    ]
);
impl_actions!(
    vim,
    [NextWordStart, NextWordEnd, PreviousWordStart, RepeatFind]
);

pub fn init(cx: &mut AppContext) {
    cx.add_action(|_: &mut Workspace, _: &Left, cx: _| motion(Motion::Left, cx));
    cx.add_action(|_: &mut Workspace, _: &Backspace, cx: _| motion(Motion::Backspace, cx));
    cx.add_action(|_: &mut Workspace, _: &Down, cx: _| motion(Motion::Down, cx));
    cx.add_action(|_: &mut Workspace, _: &Up, cx: _| motion(Motion::Up, cx));
    cx.add_action(|_: &mut Workspace, _: &Right, cx: _| motion(Motion::Right, cx));
    cx.add_action(|_: &mut Workspace, _: &FirstNonWhitespace, cx: _| {
        motion(Motion::FirstNonWhitespace, cx)
    });
    cx.add_action(|_: &mut Workspace, _: &StartOfLine, cx: _| motion(Motion::StartOfLine, cx));
    cx.add_action(|_: &mut Workspace, _: &EndOfLine, cx: _| motion(Motion::EndOfLine, cx));
    cx.add_action(|_: &mut Workspace, _: &CurrentLine, cx: _| motion(Motion::CurrentLine, cx));
    cx.add_action(|_: &mut Workspace, _: &StartOfParagraph, cx: _| {
        motion(Motion::StartOfParagraph, cx)
    });
    cx.add_action(|_: &mut Workspace, _: &EndOfParagraph, cx: _| {
        motion(Motion::EndOfParagraph, cx)
    });
    cx.add_action(|_: &mut Workspace, _: &StartOfDocument, cx: _| {
        motion(Motion::StartOfDocument, cx)
    });
    cx.add_action(|_: &mut Workspace, _: &EndOfDocument, cx: _| motion(Motion::EndOfDocument, cx));
    cx.add_action(|_: &mut Workspace, _: &Matching, cx: _| motion(Motion::Matching, cx));

    cx.add_action(
        |_: &mut Workspace, &NextWordStart { ignore_punctuation }: &NextWordStart, cx: _| {
            motion(Motion::NextWordStart { ignore_punctuation }, cx)
        },
    );
    cx.add_action(
        |_: &mut Workspace, &NextWordEnd { ignore_punctuation }: &NextWordEnd, cx: _| {
            motion(Motion::NextWordEnd { ignore_punctuation }, cx)
        },
    );
    cx.add_action(
        |_: &mut Workspace,
         &PreviousWordStart { ignore_punctuation }: &PreviousWordStart,
         cx: _| { motion(Motion::PreviousWordStart { ignore_punctuation }, cx) },
    );
    cx.add_action(|_: &mut Workspace, &NextLineStart, cx: _| motion(Motion::NextLineStart, cx));
    cx.add_action(|_: &mut Workspace, action: &RepeatFind, cx: _| {
        repeat_motion(action.backwards, cx)
    })
}

pub(crate) fn motion(motion: Motion, cx: &mut WindowContext) {
    if let Some(Operator::FindForward { .. }) | Some(Operator::FindBackward { .. }) =
        Vim::read(cx).active_operator()
    {
        Vim::update(cx, |vim, cx| vim.pop_operator(cx));
    }

    let times = Vim::update(cx, |vim, cx| vim.pop_number_operator(cx));
    let operator = Vim::read(cx).active_operator();
    match Vim::read(cx).state().mode {
        Mode::Normal => normal_motion(motion, operator, times, cx),
        Mode::Visual | Mode::VisualLine | Mode::VisualBlock => visual_motion(motion, times, cx),
        Mode::Insert => {
            // Shouldn't execute a motion in insert mode. Ignoring
        }
    }
    Vim::update(cx, |vim, cx| vim.clear_operator(cx));
}

fn repeat_motion(backwards: bool, cx: &mut WindowContext) {
    let find = match Vim::read(cx).workspace_state.last_find.clone() {
        Some(Motion::FindForward { before, text }) => {
            if backwards {
                Motion::FindBackward {
                    after: before,
                    text,
                }
            } else {
                Motion::FindForward { before, text }
            }
        }

        Some(Motion::FindBackward { after, text }) => {
            if backwards {
                Motion::FindForward {
                    before: after,
                    text,
                }
            } else {
                Motion::FindBackward { after, text }
            }
        }
        _ => return,
    };

    motion(find, cx)
}

// Motion handling is specified here:
// https://github.com/vim/vim/blob/master/runtime/doc/motion.txt
impl Motion {
    pub fn linewise(&self) -> bool {
        use Motion::*;
        match self {
            Down | Up | StartOfDocument | EndOfDocument | CurrentLine | NextLineStart
            | StartOfParagraph | EndOfParagraph => true,
            EndOfLine
            | NextWordEnd { .. }
            | Matching
            | FindForward { .. }
            | Left
            | Backspace
            | Right
            | StartOfLine
            | NextWordStart { .. }
            | PreviousWordStart { .. }
            | FirstNonWhitespace
            | FindBackward { .. } => false,
        }
    }

    pub fn infallible(&self) -> bool {
        use Motion::*;
        match self {
            StartOfDocument | EndOfDocument | CurrentLine => true,
            Down
            | Up
            | EndOfLine
            | NextWordEnd { .. }
            | Matching
            | FindForward { .. }
            | Left
            | Backspace
            | Right
            | StartOfLine
            | StartOfParagraph
            | EndOfParagraph
            | NextWordStart { .. }
            | PreviousWordStart { .. }
            | FirstNonWhitespace
            | FindBackward { .. }
            | NextLineStart => false,
        }
    }

    pub fn inclusive(&self) -> bool {
        use Motion::*;
        match self {
            Down
            | Up
            | StartOfDocument
            | EndOfDocument
            | CurrentLine
            | EndOfLine
            | NextWordEnd { .. }
            | Matching
            | FindForward { .. }
            | NextLineStart => true,
            Left
            | Backspace
            | Right
            | StartOfLine
            | StartOfParagraph
            | EndOfParagraph
            | NextWordStart { .. }
            | PreviousWordStart { .. }
            | FirstNonWhitespace
            | FindBackward { .. } => false,
        }
    }

    pub fn move_point(
        &self,
        map: &DisplaySnapshot,
        point: DisplayPoint,
        goal: SelectionGoal,
        maybe_times: Option<usize>,
    ) -> Option<(DisplayPoint, SelectionGoal)> {
        let times = maybe_times.unwrap_or(1);
        use Motion::*;
        let infallible = self.infallible();
        let (new_point, goal) = match self {
            Left => (left(map, point, times), SelectionGoal::None),
            Backspace => (backspace(map, point, times), SelectionGoal::None),
            Down => down(map, point, goal, times),
            Up => up(map, point, goal, times),
            Right => (right(map, point, times), SelectionGoal::None),
            NextWordStart { ignore_punctuation } => (
                next_word_start(map, point, *ignore_punctuation, times),
                SelectionGoal::None,
            ),
            NextWordEnd { ignore_punctuation } => (
                next_word_end(map, point, *ignore_punctuation, times),
                SelectionGoal::None,
            ),
            PreviousWordStart { ignore_punctuation } => (
                previous_word_start(map, point, *ignore_punctuation, times),
                SelectionGoal::None,
            ),
            FirstNonWhitespace => (first_non_whitespace(map, point), SelectionGoal::None),
            StartOfLine => (start_of_line(map, point), SelectionGoal::None),
            EndOfLine => (end_of_line(map, point), SelectionGoal::None),
            StartOfParagraph => (
                movement::start_of_paragraph(map, point, times),
                SelectionGoal::None,
            ),
            EndOfParagraph => (
                map.clip_at_line_end(movement::end_of_paragraph(map, point, times)),
                SelectionGoal::None,
            ),
            CurrentLine => (end_of_line(map, point), SelectionGoal::None),
            StartOfDocument => (start_of_document(map, point, times), SelectionGoal::None),
            EndOfDocument => (
                end_of_document(map, point, maybe_times),
                SelectionGoal::None,
            ),
            Matching => (matching(map, point), SelectionGoal::None),
            FindForward { before, text } => (
                find_forward(map, point, *before, text.clone(), times),
                SelectionGoal::None,
            ),
            FindBackward { after, text } => (
                find_backward(map, point, *after, text.clone(), times),
                SelectionGoal::None,
            ),
            NextLineStart => (next_line_start(map, point, times), SelectionGoal::None),
        };

        (new_point != point || infallible).then_some((new_point, goal))
    }

    // Expands a selection using self motion for an operator
    pub fn expand_selection(
        &self,
        map: &DisplaySnapshot,
        selection: &mut Selection<DisplayPoint>,
        times: Option<usize>,
        expand_to_surrounding_newline: bool,
    ) -> bool {
        if let Some((new_head, goal)) =
            self.move_point(map, selection.head(), selection.goal, times)
        {
            selection.set_head(new_head, goal);

            if self.linewise() {
                selection.start = map.prev_line_boundary(selection.start.to_point(map)).1;

                if expand_to_surrounding_newline {
                    if selection.end.row() < map.max_point().row() {
                        *selection.end.row_mut() += 1;
                        *selection.end.column_mut() = 0;
                        selection.end = map.clip_point(selection.end, Bias::Right);
                        // Don't reset the end here
                        return true;
                    } else if selection.start.row() > 0 {
                        *selection.start.row_mut() -= 1;
                        *selection.start.column_mut() = map.line_len(selection.start.row());
                        selection.start = map.clip_point(selection.start, Bias::Left);
                    }
                }

                (_, selection.end) = map.next_line_boundary(selection.end.to_point(map));
            } else {
                // If the motion is exclusive and the end of the motion is in column 1, the
                // end of the motion is moved to the end of the previous line and the motion
                // becomes inclusive. Example: "}" moves to the first line after a paragraph,
                // but "d}" will not include that line.
                let mut inclusive = self.inclusive();
                if !inclusive
                    && self != &Motion::Backspace
                    && selection.end.row() > selection.start.row()
                    && selection.end.column() == 0
                {
                    inclusive = true;
                    *selection.end.row_mut() -= 1;
                    *selection.end.column_mut() = 0;
                    selection.end = map.clip_point(
                        map.next_line_boundary(selection.end.to_point(map)).1,
                        Bias::Left,
                    );
                }

                if inclusive && selection.end.column() < map.line_len(selection.end.row()) {
                    *selection.end.column_mut() += 1;
                }
            }
            true
        } else {
            false
        }
    }
}

fn left(map: &DisplaySnapshot, mut point: DisplayPoint, times: usize) -> DisplayPoint {
    for _ in 0..times {
        point = movement::saturating_left(map, point);
        if point.column() == 0 {
            break;
        }
    }
    point
}

fn backspace(map: &DisplaySnapshot, mut point: DisplayPoint, times: usize) -> DisplayPoint {
    for _ in 0..times {
        point = movement::left(map, point);
    }
    point
}

fn down(
    map: &DisplaySnapshot,
    mut point: DisplayPoint,
    mut goal: SelectionGoal,
    times: usize,
) -> (DisplayPoint, SelectionGoal) {
    for _ in 0..times {
        (point, goal) = movement::down(map, point, goal, true);
    }
    (point, goal)
}

fn up(
    map: &DisplaySnapshot,
    mut point: DisplayPoint,
    mut goal: SelectionGoal,
    times: usize,
) -> (DisplayPoint, SelectionGoal) {
    for _ in 0..times {
        (point, goal) = movement::up(map, point, goal, true);
    }
    (point, goal)
}

pub(crate) fn right(map: &DisplaySnapshot, mut point: DisplayPoint, times: usize) -> DisplayPoint {
    for _ in 0..times {
        let new_point = movement::saturating_right(map, point);
        if point == new_point {
            break;
        }
        point = new_point;
    }
    point
}

pub(crate) fn next_word_start(
    map: &DisplaySnapshot,
    mut point: DisplayPoint,
    ignore_punctuation: bool,
    times: usize,
) -> DisplayPoint {
    for _ in 0..times {
        let mut crossed_newline = false;
        point = movement::find_boundary(map, point, |left, right| {
            let left_kind = char_kind(left).coerce_punctuation(ignore_punctuation);
            let right_kind = char_kind(right).coerce_punctuation(ignore_punctuation);
            let at_newline = right == '\n';

            let found = (left_kind != right_kind && right_kind != CharKind::Whitespace)
                || at_newline && crossed_newline
                || at_newline && left == '\n'; // Prevents skipping repeated empty lines

            crossed_newline |= at_newline;
            found
        })
    }
    point
}

fn next_word_end(
    map: &DisplaySnapshot,
    mut point: DisplayPoint,
    ignore_punctuation: bool,
    times: usize,
) -> DisplayPoint {
    for _ in 0..times {
        *point.column_mut() += 1;
        point = movement::find_boundary(map, point, |left, right| {
            let left_kind = char_kind(left).coerce_punctuation(ignore_punctuation);
            let right_kind = char_kind(right).coerce_punctuation(ignore_punctuation);

            left_kind != right_kind && left_kind != CharKind::Whitespace
        });

        // find_boundary clips, so if the character after the next character is a newline or at the end of the document, we know
        // we have backtracked already
        if !map
            .chars_at(point)
            .nth(1)
            .map(|(c, _)| c == '\n')
            .unwrap_or(true)
        {
            *point.column_mut() = point.column().saturating_sub(1);
        }
        point = map.clip_point(point, Bias::Left);
    }
    point
}

fn previous_word_start(
    map: &DisplaySnapshot,
    mut point: DisplayPoint,
    ignore_punctuation: bool,
    times: usize,
) -> DisplayPoint {
    for _ in 0..times {
        // This works even though find_preceding_boundary is called for every character in the line containing
        // cursor because the newline is checked only once.
        point = movement::find_preceding_boundary(map, point, |left, right| {
            let left_kind = char_kind(left).coerce_punctuation(ignore_punctuation);
            let right_kind = char_kind(right).coerce_punctuation(ignore_punctuation);

            (left_kind != right_kind && !right.is_whitespace()) || left == '\n'
        });
    }
    point
}

fn first_non_whitespace(map: &DisplaySnapshot, from: DisplayPoint) -> DisplayPoint {
    let mut last_point = DisplayPoint::new(from.row(), 0);
    for (ch, point) in map.chars_at(last_point) {
        if ch == '\n' {
            return from;
        }

        last_point = point;

        if char_kind(ch) != CharKind::Whitespace {
            break;
        }
    }

    map.clip_point(last_point, Bias::Left)
}

fn start_of_line(map: &DisplaySnapshot, point: DisplayPoint) -> DisplayPoint {
    map.prev_line_boundary(point.to_point(map)).1
}

fn end_of_line(map: &DisplaySnapshot, point: DisplayPoint) -> DisplayPoint {
    map.clip_point(map.next_line_boundary(point.to_point(map)).1, Bias::Left)
}

fn start_of_document(map: &DisplaySnapshot, point: DisplayPoint, line: usize) -> DisplayPoint {
    let mut new_point = Point::new((line - 1) as u32, 0).to_display_point(map);
    *new_point.column_mut() = point.column();
    map.clip_point(new_point, Bias::Left)
}

fn end_of_document(
    map: &DisplaySnapshot,
    point: DisplayPoint,
    line: Option<usize>,
) -> DisplayPoint {
    let new_row = if let Some(line) = line {
        (line - 1) as u32
    } else {
        map.max_buffer_row()
    };

    let new_point = Point::new(new_row, point.column());
    map.clip_point(new_point.to_display_point(map), Bias::Left)
}

fn matching(map: &DisplaySnapshot, display_point: DisplayPoint) -> DisplayPoint {
    // https://github.com/vim/vim/blob/1d87e11a1ef201b26ed87585fba70182ad0c468a/runtime/doc/motion.txt#L1200
    let point = display_point.to_point(map);
    let offset = point.to_offset(&map.buffer_snapshot);

    // Ensure the range is contained by the current line.
    let mut line_end = map.next_line_boundary(point).0;
    if line_end == point {
        line_end = map.max_point().to_point(map);
    }

    let line_range = map.prev_line_boundary(point).0..line_end;
    let visible_line_range =
        line_range.start..Point::new(line_range.end.row, line_range.end.column.saturating_sub(1));
    let ranges = map
        .buffer_snapshot
        .bracket_ranges(visible_line_range.clone());
    if let Some(ranges) = ranges {
        let line_range = line_range.start.to_offset(&map.buffer_snapshot)
            ..line_range.end.to_offset(&map.buffer_snapshot);
        let mut closest_pair_destination = None;
        let mut closest_distance = usize::MAX;

        for (open_range, close_range) in ranges {
            if open_range.start >= offset && line_range.contains(&open_range.start) {
                let distance = open_range.start - offset;
                if distance < closest_distance {
                    closest_pair_destination = Some(close_range.start);
                    closest_distance = distance;
                    continue;
                }
            }

            if close_range.start >= offset && line_range.contains(&close_range.start) {
                let distance = close_range.start - offset;
                if distance < closest_distance {
                    closest_pair_destination = Some(open_range.start);
                    closest_distance = distance;
                    continue;
                }
            }

            continue;
        }

        closest_pair_destination
            .map(|destination| destination.to_display_point(map))
            .unwrap_or(display_point)
    } else {
        display_point
    }
}

fn find_forward(
    map: &DisplaySnapshot,
    from: DisplayPoint,
    before: bool,
    target: Arc<str>,
    times: usize,
) -> DisplayPoint {
    map.find_while(from, target.as_ref(), |ch, _| ch != '\n')
        .skip_while(|found_at| found_at == &from)
        .nth(times - 1)
        .map(|mut found| {
            if before {
                *found.column_mut() -= 1;
                found = map.clip_point(found, Bias::Right);
                found
            } else {
                found
            }
        })
        .unwrap_or(from)
}

fn find_backward(
    map: &DisplaySnapshot,
    from: DisplayPoint,
    after: bool,
    target: Arc<str>,
    times: usize,
) -> DisplayPoint {
    map.reverse_find_while(from, target.as_ref(), |ch, _| ch != '\n')
        .skip_while(|found_at| found_at == &from)
        .nth(times - 1)
        .map(|mut found| {
            if after {
                *found.column_mut() += 1;
                found = map.clip_point(found, Bias::Left);
                found
            } else {
                found
            }
        })
        .unwrap_or(from)
}

fn next_line_start(map: &DisplaySnapshot, point: DisplayPoint, times: usize) -> DisplayPoint {
    let new_row = (point.row() + times as u32).min(map.max_buffer_row());
    first_non_whitespace(
        map,
        map.clip_point(DisplayPoint::new(new_row, 0), Bias::Left),
    )
}

#[cfg(test)]

mod test {

    use crate::test::NeovimBackedTestContext;
    use indoc::indoc;

    #[gpui::test]
    async fn test_start_end_of_paragraph(cx: &mut gpui::TestAppContext) {
        let mut cx = NeovimBackedTestContext::new(cx).await;

        let initial_state = indoc! {r"ˇabc
            def

            paragraph
            the second



            third and
            final"};

        // goes down once
        cx.set_shared_state(initial_state).await;
        cx.simulate_shared_keystrokes(["}"]).await;
        cx.assert_shared_state(indoc! {r"abc
            def
            ˇ
            paragraph
            the second



            third and
            final"})
            .await;

        // goes up once
        cx.simulate_shared_keystrokes(["{"]).await;
        cx.assert_shared_state(initial_state).await;

        // goes down twice
        cx.simulate_shared_keystrokes(["2", "}"]).await;
        cx.assert_shared_state(indoc! {r"abc
            def

            paragraph
            the second
            ˇ


            third and
            final"})
            .await;

        // goes down over multiple blanks
        cx.simulate_shared_keystrokes(["}"]).await;
        cx.assert_shared_state(indoc! {r"abc
                def

                paragraph
                the second



                third and
                finaˇl"})
            .await;

        // goes up twice
        cx.simulate_shared_keystrokes(["2", "{"]).await;
        cx.assert_shared_state(indoc! {r"abc
                def
                ˇ
                paragraph
                the second



                third and
                final"})
            .await
    }

    #[gpui::test]
    async fn test_matching(cx: &mut gpui::TestAppContext) {
        let mut cx = NeovimBackedTestContext::new(cx).await;

        cx.set_shared_state(indoc! {r"func ˇ(a string) {
                do(something(with<Types>.and_arrays[0, 2]))
            }"})
            .await;
        cx.simulate_shared_keystrokes(["%"]).await;
        cx.assert_shared_state(indoc! {r"func (a stringˇ) {
                do(something(with<Types>.and_arrays[0, 2]))
            }"})
            .await;

        // test it works on the last character of the line
        cx.set_shared_state(indoc! {r"func (a string) ˇ{
            do(something(with<Types>.and_arrays[0, 2]))
            }"})
            .await;
        cx.simulate_shared_keystrokes(["%"]).await;
        cx.assert_shared_state(indoc! {r"func (a string) {
            do(something(with<Types>.and_arrays[0, 2]))
            ˇ}"})
            .await;

        // test it works on immediate nesting
        cx.set_shared_state("ˇ{()}").await;
        cx.simulate_shared_keystrokes(["%"]).await;
        cx.assert_shared_state("{()ˇ}").await;
        cx.simulate_shared_keystrokes(["%"]).await;
        cx.assert_shared_state("ˇ{()}").await;

        // test it works on immediate nesting inside braces
        cx.set_shared_state("{\n    ˇ{()}\n}").await;
        cx.simulate_shared_keystrokes(["%"]).await;
        cx.assert_shared_state("{\n    {()ˇ}\n}").await;

        // test it jumps to the next paren on a line
        cx.set_shared_state("func ˇboop() {\n}").await;
        cx.simulate_shared_keystrokes(["%"]).await;
        cx.assert_shared_state("func boop(ˇ) {\n}").await;
    }

    #[gpui::test]
    async fn test_comma_semicolon(cx: &mut gpui::TestAppContext) {
        let mut cx = NeovimBackedTestContext::new(cx).await;

        cx.set_shared_state("ˇone two three four").await;
        cx.simulate_shared_keystrokes(["f", "o"]).await;
        cx.assert_shared_state("one twˇo three four").await;
        cx.simulate_shared_keystrokes([","]).await;
        cx.assert_shared_state("ˇone two three four").await;
        cx.simulate_shared_keystrokes(["2", ";"]).await;
        cx.assert_shared_state("one two three fˇour").await;
        cx.simulate_shared_keystrokes(["shift-t", "e"]).await;
        cx.assert_shared_state("one two threeˇ four").await;
        cx.simulate_shared_keystrokes(["3", ";"]).await;
        cx.assert_shared_state("oneˇ two three four").await;
        cx.simulate_shared_keystrokes([","]).await;
        cx.assert_shared_state("one two thˇree four").await;
    }

    #[gpui::test]
    async fn test_next_line_start(cx: &mut gpui::TestAppContext) {
        let mut cx = NeovimBackedTestContext::new(cx).await;
        cx.set_shared_state("ˇone\n  two\nthree").await;
        cx.simulate_shared_keystrokes(["enter"]).await;
        cx.assert_shared_state("one\n  ˇtwo\nthree").await;
    }
}
