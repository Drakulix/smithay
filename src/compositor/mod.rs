//! Utilities for handling surfaces, subsurfaces and regions
//!
//! This module provides the `CompositorHandler<U,H>` type, with implements
//! automatic handling of sufaces, subsurfaces and region wayland objects,
//! by being registered as a global handler for `wl_compositor` and
//! `wl_subcompositor`.
//!
//! ## Why use this handler
//!
//! This handler does a simple job: it stores in a coherent way the state of
//! surface trees with subsurfaces, to provide you a direct access to the tree
//! structure and all surface metadata.
//!
//! As such, you can, given a root surface with a role requiring it to be displayed,
//! you can iterate over the whole tree of subsurfaces to recover all the metadata you
//! need to display the subsurface tree.
//!
//! This handler will not do anything more than present you the metadata specified by the
//! client in a coherent and practical way. All the logic regarding to drawing itself, and
//! the positionning of windows (surface trees) one relative to another is out of its scope.
//!
//! ## How to use it
//!
//! ### Initialization
//!
//! To initialize this handler, simply instanciate it and register it to the event loop
//! as a global handler for wl_compositor and wl_subcompositor:
//!
//! ```
//! # extern crate wayland_server;
//! # #[macro_use] extern crate smithay;
//! use wayland_server::protocol::wl_compositor::WlCompositor;
//! use wayland_server::protocol::wl_subcompositor::WlSubcompositor;
//! use smithay::compositor;
//!
//! // Define some user data to be associated with the surfaces.
//! // It must implement the Default trait, which will represent the state of a surface which
//! // has just been created.
//! #[derive(Default)]
//! struct MyData {
//!     // whatever you need here
//! }
//!
//! // Declare the roles enum
//! define_roles!(MyRoles);
//!
//! // Define a sub-handler to take care of the events the CompositorHandler does not rack for you
//! struct MyHandler {
//!     // whatever you need
//! }
//!
//! // Implement the handler trait for this sub-handler
//! impl compositor::Handler<MyData, MyRoles> for MyHandler {
//!     // See the trait documentation for its implementation
//!     // A default implementation for each method is provided, that does nothing
//! }
//!
//! // A type alias to shorten things:
//! type MyCompositorHandler = compositor::CompositorHandler<MyData, MyRoles, MyHandler>;
//!
//! # fn main() {
//! # let (_display, mut event_loop) = wayland_server::create_display();
//!
//! // Instanciate the CompositorHandler and give it to the event loop
//! let compositor_hid = event_loop.add_handler_with_init(
//!     MyCompositorHandler::new(MyHandler{ /* ... */ }, None /* put a logger here */)
//! );
//!
//! // Register it as a handler for wl_compositor
//! event_loop.register_global::<WlCompositor, MyCompositorHandler>(compositor_hid, 4);
//!
//! // Register it as a handler for wl_subcompositor
//! event_loop.register_global::<WlSubcompositor, MyCompositorHandler>(compositor_hid, 1);
//!
//! // retrieve the token needed to access the surfaces' metadata
//! let compositor_token = {
//!     let state = event_loop.state();
//!     state.get_handler::<MyCompositorHandler>(compositor_hid).get_token()
//! };
//!
//! // You're now ready to go!
//! # }
//! ```
//!
//! ### Use the surface metadata
//!
//! As you can see in the previous example, in the end we are retrieving a token from
//! the `CompositorHandler`. This token is necessary to retrieve the metadata associated with
//! a surface. It can be cloned, and is sendable accross threads. See `CompositorToken` for
//! the details of what it enables you.
//!
//! The surface metadata is held in the `SurfaceAttributes` struct. In contains double-buffered
//! state pending from the client as defined by the protocol for wl_surface, as well as your
//! user-defined type holding any data you need to have associated with a struct. See its
//! documentation for details.
//!
//! This `CompositorToken` also provides access to the metadata associated with the role of the
//! surfaces. See the documentation of the `roles` submodule for a detailed explanation.

mod global;
mod handlers;
mod tree;
mod region;
pub mod roles;

use self::region::RegionData;
use self::roles::{Role, RoleType, WrongRole};
use self::tree::SurfaceData;
pub use self::tree::TraversalAction;
use wayland_server::{resource_is_registered, Client, EventLoopHandle, Init};

use wayland_server::protocol::{wl_buffer, wl_callback, wl_output, wl_region, wl_surface};

/// Description of which part of a surface
/// should be considered damaged and needs to be redrawn
pub enum Damage {
    /// The whole surface must be considered damaged (this is the default)
    Full,
    /// A rectangle containing the damaged zone, in surface coordinates
    Surface(Rectangle),
    /// A rectangle containing the damaged zone, in buffer coordinates
    ///
    /// Note: Buffer scaling must be taken into consideration
    Buffer(Rectangle),
}

/// Data associated with a surface, aggreged by the handlers
///
/// Most of the fields of this struct represent a double-buffered state, which
/// should only be applied once a `commit` request is received from the surface.
///
/// You are responsible for setting those values as you see fit to avoid
/// processing them two times.
pub struct SurfaceAttributes<U> {
    /// Buffer defining the contents of the surface
    ///
    /// The tuple represent the coordinates of this buffer
    /// relative to the location of the current buffer.
    ///
    /// If set to `Some(None)`, it means the user specifically asked for the
    /// surface to be unmapped.
    ///
    /// You are free to set this field to `None` to avoid processing it several
    /// times. It'll be set to `Some(...)` if the user attaches a buffer (or NULL) to
    /// the surface.
    pub buffer: Option<Option<(wl_buffer::WlBuffer, (i32, i32))>>,
    /// Scale of the contents of the buffer, for higher-resolution contents.
    ///
    /// If it matches the one of the output displaying this surface, no change
    /// is necessary.
    pub buffer_scale: i32,
    /// Transform under which interpret the contents of the buffer
    ///
    /// If it matches the one of the output displaying this surface, no change
    /// is necessary.
    pub buffer_transform: wl_output::Transform,
    /// Region of the surface that is guaranteed to be opaque
    ///
    /// By default the whole surface is potentially transparent
    pub opaque_region: Option<RegionAttributes>,
    /// Region of the surface that is sensitive to user input
    ///
    /// By default the whole surface should be sensitive
    pub input_region: Option<RegionAttributes>,
    /// Damage rectangle
    ///
    /// Hint provided by the client to suggest that only this part
    /// of the surface was changed and needs to be redrawn
    pub damage: Damage,
    /// User-controlled data
    ///
    /// This is your field to host whatever you need.
    pub user_data: U,
}

impl<U: Default> Default for SurfaceAttributes<U> {
    fn default() -> SurfaceAttributes<U> {
        SurfaceAttributes {
            buffer: None,
            buffer_scale: 1,
            buffer_transform: wl_output::Transform::Normal,
            opaque_region: None,
            input_region: None,
            damage: Damage::Full,
            user_data: Default::default(),
        }
    }
}

/// Attributes defining the behaviour of a sub-surface relative to its parent
#[derive(Copy, Clone, Debug)]
pub struct SubsurfaceRole {
    /// Horizontal location of the top-left corner of this sub-surface relative to
    /// the top-left corner of its parent
    pub x: i32,
    /// Vertical location of the top-left corner of this sub-surface relative to
    /// the top-left corner of its parent
    pub y: i32,
    /// Sync status of this sub-surface
    ///
    /// If `true`, this surface should be repainted synchronously with its parent
    /// if `false`, it should be considered independant of its parent regarding
    /// repaint timings.
    pub sync: bool,
}

impl Default for SubsurfaceRole {
    fn default() -> SubsurfaceRole {
        SubsurfaceRole {
            x: 0,
            y: 0,
            sync: true,
        }
    }
}

/// Kind of a rectangle part of a region
#[derive(Copy, Clone, Debug)]
pub enum RectangleKind {
    /// This rectangle should be added to the region
    Add,
    /// The intersection of this rectangle with the region should
    /// be removed from the region
    Subtract,
}

/// A rectangle defined by its top-left corner and dimensions
#[derive(Copy, Clone, Debug)]
pub struct Rectangle {
    /// horizontal position of the top-leftcorner of the rectangle, in surface coordinates
    pub x: i32,
    /// vertical position of the top-leftcorner of the rectangle, in surface coordinates
    pub y: i32,
    /// width of the rectangle
    pub width: i32,
    /// height of the rectangle
    pub height: i32,
}

/// Description of the contents of a region
///
/// A region is defined as an union and difference of rectangle.
///
/// This struct contains an ordered Vec containing the rectangles defining
/// a region. They should be added or substracted in this order to compute the
/// actual contents of the region.
#[derive(Clone, Debug)]
pub struct RegionAttributes {
    /// List of rectangle part of this region
    pub rects: Vec<(RectangleKind, Rectangle)>,
}

impl Default for RegionAttributes {
    fn default() -> RegionAttributes {
        RegionAttributes { rects: Vec::new() }
    }
}

/// A Compositor global token
///
/// This token can be cloned at will, and is the entry-point to
/// access data associated with the wl_surface and wl_region managed
/// by the `CompositorGlobal` that provided it.
pub struct CompositorToken<U, R, H> {
    hid: usize,
    _data: ::std::marker::PhantomData<*mut U>,
    _role: ::std::marker::PhantomData<*mut R>,
    _handler: ::std::marker::PhantomData<*mut H>,
}

unsafe impl<U: Send, R: Send, H: Send> Send for CompositorToken<U, R, H> {}
unsafe impl<U: Send, R: Send, H: Send> Sync for CompositorToken<U, R, H> {}

// we implement them manually because #[derive(..)] would require
// U: Clone and H: Clone ...
impl<U, R, H> Copy for CompositorToken<U, R, H> {}
impl<U, R, H> Clone for CompositorToken<U, R, H> {
    fn clone(&self) -> CompositorToken<U, R, H> {
        *self
    }
}

impl<U: Send + 'static, R: Send + 'static, H: Handler<U, R> + Send + 'static> CompositorToken<U, R, H> {
    /// Access the data of a surface
    ///
    /// The closure will be called with the contents of the data associated with this surface.
    ///
    /// If the surface is not managed by the CompositorGlobal that provided this token, this
    /// will panic (having more than one compositor is not supported).
    pub fn with_surface_data<F, T>(&self, surface: &wl_surface::WlSurface, f: F) -> T
    where
        F: FnOnce(&mut SurfaceAttributes<U>) -> T,
    {
        assert!(
            resource_is_registered::<_, CompositorHandler<U, R, H>>(surface, self.hid),
            "Accessing the data of foreign surfaces is not supported."
        );
        unsafe { SurfaceData::<U, R>::with_data(surface, f) }
    }
}

impl<U, R, H> CompositorToken<U, R, H>
where
    U: Send + 'static,
    R: RoleType + Role<SubsurfaceRole> + Send + 'static,
    H: Handler<U, R> + Send + 'static,
{
    /// Access the data of a surface tree
    ///
    /// The provided closure is called successively on the surface and all its child subsurfaces,
    /// in a depth-first order. This matches the order in which the surfaces are supposed to be
    /// drawn: top-most last.
    ///
    /// The arguments provided to the closure are, in this order:
    ///
    /// - The surface object itself
    /// - a mutable reference to its surface attribute data
    /// - a mutable reference to its role data,
    /// - a custom value that is passer in a fold-like maneer, but only from the output of a parent
    ///   to its children. See `TraversalAction` for details.
    ///
    /// If the surface not managed by the CompositorGlobal that provided this token, this
    /// will panic (having more than one compositor is not supported).
    pub fn with_surface_tree<F, T>(&self, surface: &wl_surface::WlSurface, initial: T, f: F) -> Result<(), ()>
    where
        F: FnMut(&wl_surface::WlSurface, &mut SurfaceAttributes<U>, &mut R, &T)
              -> TraversalAction<T>,
    {
        assert!(
            resource_is_registered::<_, CompositorHandler<U, R, H>>(surface, self.hid),
            "Accessing the data of foreign surfaces is not supported."
        );
        unsafe {
            SurfaceData::<U, R>::map_tree(surface, initial, f);
        }
        Ok(())
    }

    /// Retrieve the parent of this surface
    ///
    /// Returns `None` is this surface is a root surface
    ///
    /// If the surface is not managed by the CompositorGlobal that provided this token, this
    /// will panic (having more than one compositor is not supported).
    pub fn get_parent(&self, surface: &wl_surface::WlSurface) -> Option<wl_surface::WlSurface> {
        assert!(
            resource_is_registered::<_, CompositorHandler<U, R, H>>(surface, self.hid),
            "Accessing the data of foreign surfaces is not supported."
        );
        unsafe { SurfaceData::<U, R>::get_parent(surface) }
    }

    /// Retrieve the children of this surface
    ///
    /// If the surface is not managed by the CompositorGlobal that provided this token, this
    /// will panic (having more than one compositor is not supported).
    pub fn get_children(&self, surface: &wl_surface::WlSurface) -> Vec<wl_surface::WlSurface> {
        assert!(
            resource_is_registered::<_, CompositorHandler<U, R, H>>(surface, self.hid),
            "Accessing the data of foreign surfaces is not supported."
        );
        unsafe { SurfaceData::<U, R>::get_children(surface) }
    }
}

impl<U: Send + 'static, R: Send + RoleType + 'static, H: Handler<U, R> + Send + 'static>
    CompositorToken<U, R, H> {
    /// Check wether this surface as a role or not
    ///
    /// If the surface is not managed by the CompositorGlobal that provided this token, this
    /// will panic (having more than one compositor is not supported).
    pub fn has_a_role(&self, surface: &wl_surface::WlSurface) -> bool {
        assert!(
            resource_is_registered::<_, CompositorHandler<U, R, H>>(surface, self.hid),
            "Accessing the data of foreign surfaces is not supported."
        );
        unsafe { SurfaceData::<U, R>::has_a_role(surface) }
    }

    /// Check wether this surface as a specific role
    ///
    /// If the surface is not managed by the CompositorGlobal that provided this token, this
    /// will panic (having more than one compositor is not supported).
    pub fn has_role<RoleData>(&self, surface: &wl_surface::WlSurface) -> bool
    where
        R: Role<RoleData>,
    {
        assert!(
            resource_is_registered::<_, CompositorHandler<U, R, H>>(surface, self.hid),
            "Accessing the data of foreign surfaces is not supported."
        );
        unsafe { SurfaceData::<U, R>::has_role::<RoleData>(surface) }
    }


    /// Register that this surface has given role with default data
    ///
    /// Fails if the surface already has a role.
    ///
    /// If the surface is not managed by the CompositorGlobal that provided this token, this
    /// will panic (having more than one compositor is not supported).
    pub fn give_role<RoleData>(&self, surface: &wl_surface::WlSurface) -> Result<(), ()>
    where
        R: Role<RoleData>,
        RoleData: Default,
    {
        assert!(
            resource_is_registered::<_, CompositorHandler<U, R, H>>(surface, self.hid),
            "Accessing the data of foreign surfaces is not supported."
        );
        unsafe { SurfaceData::<U, R>::give_role::<RoleData>(surface) }
    }

    /// Register that this surface has given role with given data
    ///
    /// Fails if the surface already has a role and returns the data.
    ///
    /// If the surface is not managed by the CompositorGlobal that provided this token, this
    /// will panic (having more than one compositor is not supported).
    pub fn give_role_with<RoleData>(&self, surface: &wl_surface::WlSurface, data: RoleData)
                                    -> Result<(), RoleData>
    where
        R: Role<RoleData>,
    {
        assert!(
            resource_is_registered::<_, CompositorHandler<U, R, H>>(surface, self.hid),
            "Accessing the data of foreign surfaces is not supported."
        );
        unsafe { SurfaceData::<U, R>::give_role_with::<RoleData>(surface, data) }
    }

    /// Access the role data of a surface
    ///
    /// Fails and don't call the closure if the surface doesn't have this role
    ///
    /// If the surface is not managed by the CompositorGlobal that provided this token, this
    /// will panic (having more than one compositor is not supported).
    pub fn with_role_data<RoleData, F, T>(&self, surface: &wl_surface::WlSurface, f: F)
                                          -> Result<T, WrongRole>
    where
        R: Role<RoleData>,
        F: FnOnce(&mut RoleData) -> T,
    {
        assert!(
            resource_is_registered::<_, CompositorHandler<U, R, H>>(surface, self.hid),
            "Accessing the data of foreign surfaces is not supported."
        );
        unsafe { SurfaceData::<U, R>::with_role_data::<RoleData, _, _>(surface, f) }
    }

    /// Register that this surface does not have a role any longer and retrieve the data
    ///
    /// Fails if the surface didn't already have this role.
    ///
    /// If the surface is not managed by the CompositorGlobal that provided this token, this
    /// will panic (having more than one compositor is not supported).
    pub fn remove_role<RoleData>(&self, surface: &wl_surface::WlSurface) -> Result<RoleData, WrongRole>
    where
        R: Role<RoleData>,
    {
        assert!(
            resource_is_registered::<_, CompositorHandler<U, R, H>>(surface, self.hid),
            "Accessing the data of foreign surfaces is not supported."
        );
        unsafe { SurfaceData::<U, R>::remove_role::<RoleData>(surface) }
    }

    /// Retrieve the metadata associated with a wl_region
    ///
    /// If the region is not managed by the CompositorGlobal that provided this token, this
    /// will panic (having more than one compositor is not supported).
    pub fn get_region_attributes(&self, region: &wl_region::WlRegion) -> RegionAttributes {
        assert!(
            resource_is_registered::<_, CompositorHandler<U, R, H>>(region, self.hid),
            "Accessing the data of foreign regions is not supported."
        );
        unsafe { RegionData::get_attributes(region) }
    }
}

/// A struct handling the `wl_compositor` and `wl_subcompositor` globals
///
/// It allows you to choose a custom `U` type to store data you want
/// associated with the surfaces in their metadata, as well a providing
/// a sub-handler to handle the events defined by the `Handler` trait
/// defined in this module.
///
/// See the module-level documentation for instructions and examples of use.
pub struct CompositorHandler<U, R, H> {
    my_id: usize,
    log: ::slog::Logger,
    handler: H,
    _role: ::std::marker::PhantomData<R>,
    _data: ::std::marker::PhantomData<U>,
}

impl<U, R, H> Init for CompositorHandler<U, R, H> {
    fn init(&mut self, _evqh: &mut EventLoopHandle, index: usize) {
        self.my_id = index;
        debug!(self.log, "Init finished")
    }
}

impl<U, R, H> CompositorHandler<U, R, H> {
    /// Create a new CompositorHandler
    pub fn new<L>(handler: H, logger: L) -> CompositorHandler<U, R, H>
    where
        L: Into<Option<::slog::Logger>>,
    {
        let log = ::slog_or_stdlog(logger);
        CompositorHandler {
            my_id: ::std::usize::MAX,
            log: log.new(o!("smithay_module" => "compositor_handler")),
            handler: handler,
            _role: ::std::marker::PhantomData,
            _data: ::std::marker::PhantomData,
        }
    }

    /// Create a token to access the data associated to the objects managed by this handler.
    pub fn get_token(&self) -> CompositorToken<U, R, H> {
        assert!(
            self.my_id != ::std::usize::MAX,
            "CompositorHandler is not initialized yet."
        );
        trace!(self.log, "Creating a compositor token.");
        CompositorToken {
            hid: self.my_id,
            _data: ::std::marker::PhantomData,
            _role: ::std::marker::PhantomData,
            _handler: ::std::marker::PhantomData,
        }
    }

    /// Access the underlying sub-handler
    pub fn get_handler(&mut self) -> &mut H {
        &mut self.handler
    }
}

/// Sub-handler trait for surface event handling
///
/// The global provided by Smithay cannot process these events for you, so they
/// are forwarded directly to a handler implementing this trait that you must provide
/// at creation of the `CompositorHandler`.
#[allow(unused_variables)]
pub trait Handler<U, R>: Sized {
    /// The double-buffered state has been validated by the client
    ///
    /// At this point, the pending state that has been accumulated in the `SurfaceAttributes` associated
    /// to this surface should be integrated into the current state of the surface.
    ///
    /// See [`wayland_server::protocol::wl_surface::Handler::commit`](https://docs.rs/wayland-server/*/wayland_server/protocol/wl_surface/trait.Handler.html#method.commit)
    /// for more details
    fn commit(&mut self, evlh: &mut EventLoopHandle, client: &Client, surface: &wl_surface::WlSurface,
              token: CompositorToken<U, R, Self>) {
    }
    /// The client asks to be notified when would be a good time to update the contents of this surface
    ///
    /// You must keep the provided `WlCallback` and trigger it at the appropriate time by calling
    /// its `done()` method.
    ///
    /// See [`wayland_server::protocol::wl_surface::Handler::frame`](https://docs.rs/wayland-server/*/wayland_server/protocol/wl_surface/trait.Handler.html#method.frame)
    /// for more details
    fn frame(&mut self, evlh: &mut EventLoopHandle, client: &Client, surface: &wl_surface::WlSurface,
             callback: wl_callback::WlCallback, token: CompositorToken<U, R, Self>) {
    }
}
