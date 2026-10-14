//! What the daemon API does, apart from reading requests and writing responses. Each
//! service stands beside the [`SessionController`](crate::control::SessionController):
//! it reads and writes the Store, calls the Plugins, and wakes whoever waits on what it
//! wrote.

mod actor;
mod browser;
mod message;
mod session;
mod token;
mod workspace;

pub use actor::{ActorDeclaration, ActorError, ActorService, ScheduleRequest};
pub use browser::{BrowserService, Paired, credential};
pub use message::{MailboxError, MessageError, MessageService};
pub use session::SessionService;
pub use token::TokenService;
pub use workspace::WorkspaceService;
