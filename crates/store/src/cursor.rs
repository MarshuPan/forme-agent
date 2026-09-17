use std::{collections::VecDeque, sync::Arc};

use forme_protocol as p;

use crate::sqlite::StoreCore;

pub struct EventCursor {
    store: Arc<StoreCore>,
    run_id: p::RunId,
    next_seq: u64,
    page_size: usize,
    buffered: VecDeque<p::Result<p::Event>>,
    exhausted: bool,
}

impl EventCursor {
    pub(crate) fn new(store: Arc<StoreCore>, run_id: p::RunId, page_size: usize) -> Self {
        Self {
            store,
            run_id,
            next_seq: 1,
            page_size,
            buffered: VecDeque::new(),
            exhausted: false,
        }
    }
}

impl Iterator for EventCursor {
    type Item = p::Result<p::Event>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(event) = self.buffered.pop_front() {
                return Some(event);
            }
            if self.exhausted {
                return None;
            }

            match self
                .store
                .load_page(&self.run_id, self.next_seq, self.page_size)
            {
                Ok(page) if page.items.is_empty() => {
                    self.exhausted = true;
                    return None;
                }
                Ok(page) => {
                    self.next_seq = page.next_seq;
                    self.buffered = page.items;
                }
                Err(error) => {
                    self.exhausted = true;
                    return Some(Err(error));
                }
            }
        }
    }
}

pub(crate) struct CursorPage {
    pub(crate) next_seq: u64,
    pub(crate) items: VecDeque<p::Result<p::Event>>,
}
