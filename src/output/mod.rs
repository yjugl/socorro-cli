// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

pub mod compact;
pub mod json;
pub mod markdown;

use clap::ValueEnum;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Compact,
    Json,
    Markdown,
}
