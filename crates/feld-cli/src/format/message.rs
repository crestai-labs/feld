//! Messages as a person reads them.

use feld_core::model::{Message, MessageQueued, Requester};

use crate::Context;
use crate::output::kinded;

pub fn print_message_summary(ctx: &Context, message: &Message) {
    println!("  message:      {}", message.id);
    println!(
        "  from:         {}",
        message.from.name_in(&message.to.workspace)
    );
    println!("  to:           {}", recipient(message));
    if let Some(label) = &message.label {
        println!("  label:        {label}");
    }
    println!("  accepted:     {}", ctx.local_time(&message.accepted));
    println!(
        "  state:        {}",
        message_state(message.waiting, message.queued.as_ref())
    );
    println!("  anchor:       {}", message.anchor);
}

/// The recipient as the sender reads it: qualified when the sender is of another Workspace.
fn recipient(message: &Message) -> String {
    match message.from.workspace() {
        Some(workspace) => message.to.name_in(workspace),
        None => message.to.name.clone(),
    }
}

/// What a Message waits for: a read (`waiting`), a Runtime that holds it in a queue of its
/// own to take it in (`queued`, which is waiting too), or nothing (`read`).
pub fn message_state(waiting: bool, queued: Option<&MessageQueued>) -> &'static str {
    match (waiting, queued) {
        (false, _) => "read",
        (true, Some(_)) => "queued",
        (true, None) => "waiting",
    }
}

pub fn print_message(ctx: &Context, message: &Message) {
    println!("Message");
    println!("  message:      {}", message.id);
    let from = message.from.name_in(&message.to.workspace);
    let from = match (&message.from, &message.from_session_id) {
        (Requester::Actor { .. }, Some(session)) => kinded(&from, session),
        _ => from,
    };
    println!("  from:         {from}");
    println!("  to:           {}", recipient(message));
    if let Some(label) = &message.label {
        println!("  label:        {label}");
    }
    println!("  accepted:     {}", ctx.local_time(&message.accepted));
    if let Some(read) = &message.read {
        println!(
            "  read:         {} ({})",
            ctx.local_time(&read.at),
            read.session_id
        );
    } else if message.deleted_at.is_some() {
        println!("  read:         - (deleted)");
    } else if let Some(queued) = &message.queued {
        println!(
            "  read:         - (queued {} by {})",
            ctx.local_time(&queued.at),
            queued.session_id
        );
    } else {
        println!("  read:         - (waiting)");
    }
    println!("  anchor:       {}", message.anchor);
    if let Some(deleted_at) = &message.deleted_at {
        println!("  deleted_at:   {}", ctx.local_time(deleted_at));
    }
    println!("  body:");
    for line in message.body.lines() {
        println!("    {line}");
    }
}
