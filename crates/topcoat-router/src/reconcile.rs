use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
};

const START: &str = "<!--topcoat-boundary ";
const END: &str = "<!--/topcoat-boundary ";

pub(crate) const SWAP_SCRIPT: &str = r"<script data-topcoat-stream>(function(){function comments(){var w=document.createTreeWalker(document,NodeFilter.SHOW_COMMENT),m=new Map(),n;while(n=w.nextNode()){var t=n.data;if(t.startsWith('topcoat-boundary '))m.set('s:'+t.slice(17),n);else if(t.startsWith('/topcoat-boundary '))m.set('e:'+t.slice(18),n)}return m}function apply(t){var r=t.dataset.topcoatRedirect;if(r){location.assign(r);t.remove();return}var id=t.dataset.topcoatSwap;if(!id)return;if(id==='root'){var d=new DOMParser().parseFromString(t.innerHTML,'text/html');document.documentElement.innerHTML=d.documentElement.innerHTML;globalThis.__topcoatScan?.(document.body,null,null);return}var m=comments(),s=m.get('s:'+id),e=m.get('e:'+id);if(!s||!e)return;for(var n=s.nextSibling;n&&n!==e;){var x=n.nextSibling;n.remove();n=x}e.before(t.content.cloneNode(true));globalThis.__topcoatScan?.(s.parentNode,s,e);t.remove()}new MutationObserver(function(rs){for(var r of rs)for(var n of r.addedNodes){if(n.nodeType===1&&n.matches('template[data-topcoat-swap],template[data-topcoat-redirect]'))apply(n);if(n.querySelectorAll)n.querySelectorAll('template[data-topcoat-swap],template[data-topcoat-redirect]').forEach(apply)}}).observe(document.documentElement,{childList:true,subtree:true});document.querySelectorAll('template[data-topcoat-swap],template[data-topcoat-redirect]').forEach(apply)})();</script>";

#[derive(Debug, Clone)]
pub(crate) struct Snapshot {
    html: String,
    boundaries: BTreeMap<String, Boundary>,
}

#[derive(Debug, Clone)]
struct Boundary {
    content: String,
    hash: u64,
    parent: Option<String>,
}

impl Snapshot {
    pub(crate) fn parse(html: String) -> Self {
        let boundaries = parse_boundaries(&html);
        Self { html, boundaries }
    }

    pub(crate) fn reconcile(&self, next: Self) -> (Self, String) {
        let root_structure_changed =
            self.boundaries.iter().any(|(id, boundary)| {
                boundary.parent.is_none() && !next.boundaries.contains_key(id)
            }) || next.boundaries.iter().any(|(id, boundary)| {
                boundary.parent.is_none() && !self.boundaries.contains_key(id)
            });
        let changed = next
            .boundaries
            .iter()
            .filter(|(id, boundary)| {
                self.boundaries.get(*id).map(|old| old.hash) != Some(boundary.hash)
            })
            .collect::<Vec<_>>();
        let chunk = if root_structure_changed {
            swap_template("root", &next.html)
        } else if changed.is_empty() {
            if self.boundaries.is_empty() && self.html != next.html {
                swap_template("root", &next.html)
            } else {
                String::new()
            }
        } else {
            changed
                .into_iter()
                .map(|(id, boundary)| swap_template(id, &boundary.content))
                .collect()
        };
        (next, chunk)
    }
}

pub(crate) fn redirect_template(location: &str) -> String {
    format!(
        "<template data-topcoat-redirect=\"{}\"></template>",
        escape_attribute(location),
    )
}

pub(crate) fn swap_template(id: &str, html: &str) -> String {
    format!(
        "<template data-topcoat-swap=\"{}\">{html}</template>",
        escape_attribute(id),
    )
}

fn parse_boundaries(html: &str) -> BTreeMap<String, Boundary> {
    #[derive(Clone)]
    struct Span {
        id: String,
        marker_start: usize,
        content_start: usize,
        content_end: usize,
        marker_end: usize,
    }

    let mut spans = Vec::new();
    let mut stack = Vec::<(String, usize, usize)>::new();
    let mut cursor = 0;
    loop {
        let start = html[cursor..].find(START).map(|offset| cursor + offset);
        let end = html[cursor..].find(END).map(|offset| cursor + offset);
        let Some((marker, opening)) = (match (start, end) {
            (Some(start), Some(end)) => Some(if start < end {
                (start, true)
            } else {
                (end, false)
            }),
            (Some(start), None) => Some((start, true)),
            (None, Some(end)) => Some((end, false)),
            (None, None) => None,
        }) else {
            break;
        };
        let prefix = if opening { START } else { END };
        let id_start = marker + prefix.len();
        let Some(id_end_relative) = html[id_start..].find("-->") else {
            break;
        };
        let id_end = id_start + id_end_relative;
        let marker_end = id_end + 3;
        let id = html[id_start..id_end].to_owned();
        if opening {
            stack.push((id, marker, marker_end));
        } else if let Some(index) = stack.iter().rposition(|(open, _, _)| open == &id) {
            let (id, marker_start, content_start) = stack.remove(index);
            spans.push(Span {
                id,
                marker_start,
                content_start,
                content_end: marker,
                marker_end,
            });
        }
        cursor = marker_end;
    }

    spans.sort_by_key(|span| span.marker_start);
    let mut boundaries = BTreeMap::new();
    for span in &spans {
        let mut normalized = String::new();
        let mut cursor = span.content_start;
        for child in &spans {
            if child.marker_start < span.content_start
                || child.marker_end > span.content_end
                || child.marker_start < cursor
            {
                continue;
            }
            normalized.push_str(&html[cursor..child.marker_start]);
            normalized.push_str("<!--topcoat-boundary-ref ");
            normalized.push_str(&child.id);
            normalized.push_str("-->");
            cursor = child.marker_end;
        }
        normalized.push_str(&html[cursor..span.content_end]);
        let content = html[span.content_start..span.content_end].to_owned();
        let parent = spans
            .iter()
            .filter(|candidate| {
                candidate.marker_start < span.marker_start && candidate.marker_end > span.marker_end
            })
            .min_by_key(|candidate| candidate.marker_end - candidate.marker_start)
            .map(|candidate| candidate.id.clone());
        boundaries.insert(
            span.id.clone(),
            Boundary {
                hash: hash(&normalized),
                content,
                parent,
            },
        );
    }
    boundaries
}

fn hash(value: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn escape_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_boundaries_become_swap_templates() {
        let first =
            Snapshot::parse("a<!--topcoat-boundary 1-->old<!--/topcoat-boundary 1-->z".into());
        let next =
            Snapshot::parse("a<!--topcoat-boundary 1-->new<!--/topcoat-boundary 1-->z".into());
        let (_, chunk) = first.reconcile(next);
        assert_eq!(chunk, "<template data-topcoat-swap=\"1\">new</template>");
    }

    #[test]
    fn pages_without_boundaries_swap_the_root() {
        let first = Snapshot::parse("old".into());
        let next = Snapshot::parse("new".into());
        let (_, chunk) = first.reconcile(next);
        assert_eq!(chunk, "<template data-topcoat-swap=\"root\">new</template>");
    }

    #[test]
    fn unchanged_html_emits_no_instruction() {
        let first = Snapshot::parse("same".into());
        let (_, chunk) = first.clone().reconcile(first);
        assert!(chunk.is_empty());
    }

    #[test]
    fn nested_changes_only_swap_the_child() {
        let first = Snapshot::parse("<!--topcoat-boundary p-->before<!--topcoat-boundary c-->old<!--/topcoat-boundary c-->after<!--/topcoat-boundary p-->".into());
        let next = Snapshot::parse("<!--topcoat-boundary p-->before<!--topcoat-boundary c-->new<!--/topcoat-boundary c-->after<!--/topcoat-boundary p-->".into());
        let (_, chunk) = first.reconcile(next);
        assert_eq!(chunk, "<template data-topcoat-swap=\"c\">new</template>");
    }
}
