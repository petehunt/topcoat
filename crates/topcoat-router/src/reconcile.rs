use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
};

const START: &str = "<!--topcoat-boundary ";
const END: &str = "<!--/topcoat-boundary ";

pub(crate) const SWAP_SCRIPT: &str = r"<script data-topcoat-stream>(function(){
var selector='template[data-topcoat-swap],template[data-topcoat-redirect]';
function comments(root){var w=document.createTreeWalker(root,NodeFilter.SHOW_COMMENT),m=new Map(),n;while(n=w.nextNode()){var t=n.data;if(t.startsWith('topcoat-boundary ')){var id=t.slice(17).split(' ')[0];m.set('s:'+id,n)}else if(t.startsWith('/topcoat-boundary '))m.set('e:'+t.slice(18),n)}return m}
function hashes(){var w=document.createTreeWalker(document,NodeFilter.SHOW_COMMENT),values=[],n;while(n=w.nextNode()){var t=n.data;if(t.startsWith('topcoat-boundary ')){var parts=t.slice(17).split(' ');if(parts[1])values.push(parts[0]+'='+parts[1])}}return values.join(',')}
function replace(start,end,fragment){for(var n=start.nextSibling;n&&n!==end;){var next=n.nextSibling;n.remove();n=next}end.before(fragment);globalThis.__topcoatScan?.(start.parentNode,start,end)}
function apply(root,template){var redirect=template.dataset.topcoatRedirect;if(redirect){location.assign(redirect);template.remove();return}var id=template.dataset.topcoatSwap;if(!id)return;if(id==='root'){var next=new DOMParser().parseFromString(template.innerHTML,'text/html');document.documentElement.innerHTML=next.documentElement.innerHTML;globalThis.__topcoatScan?.(document.body,null,null);return}var map=comments(root),start=map.get('s:'+id),end=map.get('e:'+id);if(start&&end){replace(start,end,template.content.cloneNode(true));if(template.dataset.topcoatHash)start.data='topcoat-boundary '+id+' '+template.dataset.topcoatHash}template.remove()}
async function navigate(url,push){document.documentElement.dataset.topcoatNavigating='';try{var response=await fetch(url,{headers:{Accept:'text/html','X-Topcoat-Boundaries':hashes()}});if(!response.ok)throw new Error('navigation failed: '+response.status);var holder=document.createElement('template');holder.innerHTML=await response.text();holder.content.querySelectorAll(selector).forEach(function(template){apply(document,template)});var metadata=holder.content.querySelector('template[data-topcoat-navigation]');if(metadata&&metadata.dataset.topcoatTitle)document.title=metadata.dataset.topcoatTitle;if(push)history.pushState(null,'',response.url);else if(response.url!==location.href)history.replaceState(null,'',response.url);scrollTo(0,0)}catch(error){location.assign(url)}finally{delete document.documentElement.dataset.topcoatNavigating}}
function enabled(){return document.documentElement.hasAttribute('data-topcoat-navigation')}
document.addEventListener('click',function(event){if(!enabled()||event.defaultPrevented||event.button!==0||event.metaKey||event.ctrlKey||event.shiftKey||event.altKey)return;var anchor=event.target.closest('a[href]');if(!anchor||anchor.target||anchor.hasAttribute('download'))return;var url=new URL(anchor.href,location.href);if(url.origin!==location.origin||url.protocol!=='http:'&&url.protocol!=='https:'||url.pathname===location.pathname&&url.search===location.search)return;event.preventDefault();navigate(url.href,true)});
addEventListener('popstate',function(){if(enabled())navigate(location.href,false)});
new MutationObserver(function(records){for(var record of records)for(var node of record.addedNodes){if(node.nodeType===1&&node.matches(selector))apply(document,node);if(node.querySelectorAll)node.querySelectorAll(selector).forEach(function(template){apply(document,template)})}}).observe(document.documentElement,{childList:true,subtree:true});document.querySelectorAll(selector).forEach(function(template){apply(document,template)});
})();</script>";

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
        let html = annotate_html(html, &boundaries);
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
                .map(|(id, boundary)| boundary_swap_template(id, boundary))
                .collect()
        };
        (next, chunk)
    }

    pub(crate) fn reconcile_hashes(&self, hashes: &BTreeMap<String, u64>) -> String {
        if self.boundaries.len() != hashes.len()
            || self.boundaries.keys().any(|id| !hashes.contains_key(id))
        {
            return swap_template("root", &self.html);
        }
        self.boundaries
            .iter()
            .filter(|(id, boundary)| hashes.get(*id) != Some(&boundary.hash))
            .map(|(id, boundary)| boundary_swap_template(id, boundary))
            .collect()
    }

    pub(crate) fn title(&self) -> &str {
        let Some(start) = self.html.find("<title>") else {
            return "";
        };
        let content = &self.html[start + "<title>".len()..];
        content.find("</title>").map_or("", |end| &content[..end])
    }

    pub(crate) fn html(&self) -> &str {
        &self.html
    }
}

pub(crate) fn client_hashes(value: &str) -> BTreeMap<String, u64> {
    value
        .split(',')
        .filter_map(|entry| {
            let (id, hash) = entry.split_once('=')?;
            Some((id.to_owned(), u64::from_str_radix(hash, 16).ok()?))
        })
        .collect()
}

pub(crate) fn navigation_template(title: &str) -> String {
    format!(
        "<template data-topcoat-navigation data-topcoat-title=\"{}\"></template>",
        escape_attribute(title),
    )
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

fn boundary_swap_template(id: &str, boundary: &Boundary) -> String {
    format!(
        "<template data-topcoat-swap=\"{}\" data-topcoat-hash=\"{:016x}\">{}</template>",
        escape_attribute(id),
        boundary.hash,
        boundary.content,
    )
}

fn annotate_html(mut html: String, boundaries: &BTreeMap<String, Boundary>) -> String {
    for (id, boundary) in boundaries {
        html = html.replace(
            &format!("<!--topcoat-boundary {id}-->"),
            &format!("<!--topcoat-boundary {id} {:016x}-->", boundary.hash),
        );
    }
    html
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
        let marker_value = &html[id_start..id_end];
        let id = marker_value
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned();
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
        assert!(chunk.starts_with("<template data-topcoat-swap=\"1\" data-topcoat-hash=\""));
        assert!(chunk.ends_with("\">new</template>"));
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
        assert!(chunk.starts_with("<template data-topcoat-swap=\"c\" data-topcoat-hash=\""));
        assert!(chunk.ends_with("\">new</template>"));
    }

    #[test]
    fn client_hashes_select_only_changed_boundaries() {
        let snapshot = Snapshot::parse(
            "<!--topcoat-boundary a-->same<!--/topcoat-boundary a--><!--topcoat-boundary b-->new<!--/topcoat-boundary b-->".into(),
        );
        let a = snapshot.boundaries["a"].hash;
        let chunk = snapshot.reconcile_hashes(&BTreeMap::from([
            (String::from("a"), a),
            (String::from("b"), 0),
        ]));

        assert!(!chunk.contains("data-topcoat-swap=\"a\""));
        assert!(chunk.contains("data-topcoat-swap=\"b\""));
        assert!(snapshot.html.contains(&format!(" a {a:016x}-->")));
    }

    #[test]
    fn swap_runtime_supports_server_driven_navigation() {
        assert!(SWAP_SCRIPT.contains("data-topcoat-navigation"));
        assert!(SWAP_SCRIPT.contains("history.pushState"));
        assert!(SWAP_SCRIPT.contains("fetch(url"));
        assert!(SWAP_SCRIPT.contains("X-Topcoat-Boundaries"));
        assert!(SWAP_SCRIPT.contains("dataset.topcoatHash"));
        assert!(!SWAP_SCRIPT.contains("pathname.split"));
    }
}
