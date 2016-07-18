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

use std::time::SystemTime;
use std::thread;
use std::sync::Arc;
use std::ops::Deref;
use std::sync::mpsc::{Receiver, Sender, SendError, channel};

use dbus;

use error;
use config;

pub struct Server {
	receiver: Receiver<Request>,
	sender:   Sender<Response>,
	signals:  Sender<Signal>,
}

#[derive(Debug)]
pub enum Request {
	/// Lock the screen.
	Lock,

	/// Cycle the saver.
	Cycle,

	/// Simulate user activity.
	SimulateUserActivity,

	/// Inhibit the starting of screen saving.
	Inhibit {
		application: String,
		reason:      String,
	},

	/// Remove a previous Inhibit.
	UnInhibit(u32),

	/// Throttle the resource usage of the screen saving.
	Throttle {
		application: String,
		reason:      String,
	},

	/// Remove a previous Throttle.
	UnThrottle(u32),

	/// Suspend any screen saver activity.
	Suspend {
		application: String,
		reason:      String,
	},

	/// Remove a previous Suspend.
	Resume(u32),

	/// Change the active status of the screen saver.
	SetActive(bool),

	/// Get the active status of the screen saver.
	GetActive,

	/// Get how many seconds the screen saver has been active.
	GetActiveTime,

	/// Get the idle status of the session.
	GetSessionIdle,

	/// Get how many seconds the session has been idle.
	GetSessionIdleTime,

	/// The system is preparing for sleep or coming out of sleep.
	PrepareForSleep(Option<SystemTime>),
}

#[derive(Debug)]
pub enum Response {
	Inhibit(u32),
	Throttle(u32),
	Suspend(u32),

	Active(bool),
	ActiveTime(u64),

	SessionIdle(bool),
	SessionIdleTime(u64),
}

#[derive(Debug)]
pub enum Signal {
	Active(bool),
	SessionIdle(bool),
	AuthenticationRequest(bool),
}

impl Server {
	pub fn spawn(config: config::Server) -> error::Result<Server> {
		let (sender, i_receiver) = channel();
		let (i_sender, receiver) = channel();
		let (s_sender, signals)  = channel();

		// Listen for relevant system events.
		{
			let sender = sender.clone();

			thread::spawn(move || {
				let c = dbus::Connection::get_private(dbus::BusType::System).unwrap();

				// Watch signals from SystemD for system suspend/resume.
				c.add_match("path='/org/freedesktop/login1',interface='org.freedesktop.login1.Manager',member='PrepareForSleep'").unwrap();

				for item in c.iter(1_000_000_000) {
					if let dbus::ConnectionItem::Signal(m) = item {
						match (&*m.interface().unwrap(), &*m.member().unwrap()) {
							("org.freedesktop.login1.Manager", "PrepareForSleep") => {
								if let Some(status) = m.get1() {
									sender.send(Request::PrepareForSleep(
										if status { Some(SystemTime::now()) } else { None })).unwrap();
								}
							}

							_ => ()
						}
					}
				}
			});
		}

		// DBus interface.
		{
			let sender                 = sender.clone();
			let (g_sender, g_receiver) = channel::<error::Result<()>>();

			macro_rules! ok {
				() => (
					g_sender.send(Ok(())).unwrap();
				);
			}

			macro_rules! try {
				($body:expr) => (
					match $body {
						Ok(value) => {
							value
						}

						Err(error) => {
							g_sender.send(Err(error.into())).unwrap();
							return;
						}
					}
				);

				(register $conn:expr, $name:expr) => (
					match $conn.register_name($name, dbus::NameFlag::DoNotQueue as u32) {
						Ok(dbus::RequestNameReply::Exists) => {
							g_sender.send(Err(error::DBus::AlreadyRegistered.into())).unwrap();
							return;
						}

						Err(error) => {
							g_sender.send(Err(error.into())).unwrap();
							return;
						}

						Ok(value) => {
							value
						}
					}
				);
			}

			macro_rules! catch {
				() => (
					g_receiver.recv().unwrap()
				)
			}

			thread::spawn(move || {
				let c = try!(dbus::Connection::get_private(dbus::BusType::Session));
				let f = dbus::tree::Factory::new_fn();

				try!(register c, "org.gnome.ScreenSaver");
				try!(register c, "meh.rust.ScreenSaver");
				ok!();

				let active = Arc::new(f.signal("ActiveChanged").sarg::<bool, _>("status"));
				let idle   = Arc::new(f.signal("SessionIdleChanged").sarg::<bool, _>("status"));
				let begin  = Arc::new(f.signal("AuthenticationRequestBegin"));
				let end    = Arc::new(f.signal("AuthenticationRequestEnd"));

				let tree = f.tree()
					// ScreenRuster interface.
					.add(f.object_path("/meh/rust/ScreenSaver").introspectable().add(f.interface("meh.rust.ScreenSaver")
						.add_m(f.method("Suspend", |m, _, _| {
							if config.ignore.contains("suspend") {
								return Err(dbus::tree::MethodErr::failed(&"Suspend is ignored"));
							}

							if let (Some(application), Some(reason)) = m.get2() {
								sender.send(Request::Suspend {
									application: application,
									reason:      reason
								}).unwrap();

								if let Response::Suspend(value) = receiver.recv().unwrap() {
									Ok(vec![m.method_return().append1(value)])
								}
								else {
									unreachable!();
								}
							}
							else {
								Err(dbus::tree::MethodErr::no_arg())
							}
						}).in_args(vec![dbus::Signature::make::<String>(), dbus::Signature::make::<String>()]))

						.add_m(f.method("Resume", |m, _, _| {
							if config.ignore.contains("suspend") {
								return Err(dbus::tree::MethodErr::failed(&"Suspend is ignored"));
							}

							if let Some(cookie) = m.get1() {
								sender.send(Request::Resume(cookie)).unwrap();

								Ok(vec![m.method_return()])
							}
							else {
								Err(dbus::tree::MethodErr::no_arg())
							}
						}).inarg::<u32, _>("cookie"))))

					// GNOME screensaver interface.
					.add(f.object_path("/org/gnome/ScreenSaver").introspectable().add(f.interface("org.gnome.ScreenSaver")
						.add_m(f.method("Lock", |m, _, _| {
							sender.send(Request::Lock).unwrap();

							Ok(vec![m.method_return()])
						}))

						.add_m(f.method("Cycle", |m, _, _| {
							sender.send(Request::Cycle).unwrap();

							Ok(vec![m.method_return()])
						}))

						.add_m(f.method("SimulateUserActivity", |m, _, _| {
							sender.send(Request::SimulateUserActivity).unwrap();

							Ok(vec![m.method_return()])
						}))

						.add_m(f.method("Inhibit", |m, _, _| {
							if config.ignore.contains("inhibit") {
								return Err(dbus::tree::MethodErr::failed(&"Inhibit is ignored"));
							}

							if let (Some(application), Some(reason)) = m.get2() {
								sender.send(Request::Inhibit {
									application: application,
									reason:      reason
								}).unwrap();

								if let Response::Inhibit(value) = receiver.recv().unwrap() {
									Ok(vec![m.method_return().append1(value)])
								}
								else {
									unreachable!();
								}
							}
							else {
								Err(dbus::tree::MethodErr::no_arg())
							}
						}).in_args(vec![dbus::Signature::make::<String>(), dbus::Signature::make::<String>()]))

						.add_m(f.method("UnInhibit", |m, _, _| {
							if config.ignore.contains("inhibit") {
								return Err(dbus::tree::MethodErr::failed(&"Inhibit is ignored"));
							}

							if let Some(cookie) = m.get1() {
								sender.send(Request::UnInhibit(cookie)).unwrap();

								Ok(vec![m.method_return()])
							}
							else {
								Err(dbus::tree::MethodErr::no_arg())
							}
						}).inarg::<u32, _>("cookie"))

						.add_m(f.method("Throttle", |m, _, _| {
							if config.ignore.contains("throttle") {
								return Err(dbus::tree::MethodErr::failed(&"Inhibit is ignored"));
							}

							if let (Some(application), Some(reason)) = m.get2() {
								sender.send(Request::Throttle {
									application: application,
									reason:      reason
								}).unwrap();

								if let Response::Throttle(value) = receiver.recv().unwrap() {
									Ok(vec![m.method_return().append1(value)])
								}
								else {
									unreachable!();
								}
							}
							else {
								Err(dbus::tree::MethodErr::no_arg())
							}
						}).in_args(vec![dbus::Signature::make::<String>(), dbus::Signature::make::<String>()]))

						.add_m(f.method("UnThrottle", |m, _, _| {
							if config.ignore.contains("throttle") {
								return Err(dbus::tree::MethodErr::failed(&"Inhibit is ignored"));
							}

							if let Some(cookie) = m.get1() {
								sender.send(Request::UnThrottle(cookie)).unwrap();

								Ok(vec![m.method_return()])
							}
							else {
								Err(dbus::tree::MethodErr::no_arg())
							}
						}).inarg::<u32, _>("cookie"))

						.add_m(f.method("SetActive", |m, _, _| {
							if let Some(value) = m.get1() {
								sender.send(Request::SetActive(value)).unwrap();

								Ok(vec![m.method_return()])
							}
							else {
								Err(dbus::tree::MethodErr::no_arg())
							}
						}).inarg::<bool, _>("active"))

						.add_m(f.method("GetActive", |m, _, _| {
							sender.send(Request::GetActive).unwrap();

							if let Response::Active(value) = receiver.recv().unwrap() {
								Ok(vec![m.method_return().append1(value)])
							}
							else {
								unreachable!();
							}
						}).outarg::<bool, _>("active"))

						.add_m(f.method("GetActiveTime", |m, _, _| {
							sender.send(Request::GetActiveTime).unwrap();

							if let Response::ActiveTime(time) = receiver.recv().unwrap() {
								Ok(vec![m.method_return().append1(time)])
							}
							else {
								unreachable!();
							}
						}).outarg::<u64, _>("time"))

						.add_m(f.method("GetSessionIdle", |m, _, _| {
							sender.send(Request::GetSessionIdle).unwrap();

							if let Response::SessionIdle(value) = receiver.recv().unwrap() {
								Ok(vec![m.method_return().append1(value)])
							}
							else {
								unreachable!();
							}
						}).outarg::<bool, _>("idle"))

						.add_m(f.method("GetSessionIdleTime", |m, _, _| {
							sender.send(Request::GetSessionIdleTime).unwrap();

							if let Response::SessionIdleTime(time) = receiver.recv().unwrap() {
								Ok(vec![m.method_return().append1(time)])
							}
							else {
								unreachable!();
							}
						}).outarg::<u64, _>("time"))

						.add_s_arc(active.clone())
						.add_s_arc(idle.clone())
						.add_s_arc(begin.clone())
						.add_s_arc(end.clone())));

				tree.set_registered(&c, true).unwrap();

				for item in tree.run(&c, c.iter(100)) {
					if let dbus::ConnectionItem::Nothing = item {
						while let Ok(signal) = signals.try_recv() {
							c.send(match signal {
								Signal::Active(status) => {
									active.msg().append1(status)
								}

								Signal::SessionIdle(status) => {
									idle.msg().append1(status)
								}

								Signal::AuthenticationRequest(true) => {
									begin.msg()
								}

								Signal::AuthenticationRequest(false) => {
									end.msg()
								}
							}).unwrap();
						}
					}
				}
			});

			catch!()?;
		}

		Ok(Server {
			receiver: i_receiver,
			sender:   i_sender,
			signals:  s_sender,
		})
	}

	pub fn response(&self, value: Response) -> Result<(), SendError<Response>> {
		self.sender.send(value)
	}

	pub fn signal(&self, value: Signal) -> Result<(), SendError<Signal>> {
		self.signals.send(value)
	}
}

impl Deref for Server {
	type Target = Receiver<Request>;

	fn deref(&self) -> &Receiver<Request> {
		&self.receiver
	}
}
