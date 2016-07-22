// Copyleft (ↄ) meh. <meh@schizofreni.co> | http://meh.schizofreni.co
//
// This file is part of screenruster.
//
// screenruster is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// screenruster is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with screenruster.  If not, see <http://www.gnu.org/licenses/>.

use std::sync::{Arc, RwLock};
use toml;

#[derive(Clone, Default, Debug)]
pub struct Auth(pub(super) Arc<RwLock<toml::Table>>);

impl Auth {
	/// Get the configuration for a specific authorization module.
	pub fn get<S: AsRef<str>>(&self, name: S) -> toml::Table {
		self.0.read().unwrap().get(name.as_ref()).and_then(|v| v.as_table()).cloned().unwrap_or_default()
	}
}