use bytemuck::{Pod, Zeroable};
use cgmath::Point2;

use std::{
    fmt,
    ops::{Deref, DerefMut},
    result,
};

use crate::Style;

pub trait Resize {
    fn resize(&self, offset: Location, scale_factor: f32) -> Self;
}

impl Resize for () {
    fn resize(&self, _: Location, _: f32) -> Self {
        ()
    }
}

// screen coordinate, in pixels
#[derive(Clone, Copy, Debug, Default)]
pub struct Location {
    pub x: f32,
    pub y: f32,
}

// screen coordinate, in pixels
#[derive(Clone, Copy, Debug, Default)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl From<Size> for stretch::geometry::Size<stretch::style::Dimension> {
    fn from(val: Size) -> stretch::geometry::Size<stretch::style::Dimension> {
        stretch::geometry::Size {
            width: stretch::style::Dimension::Points(val.width),
            height: stretch::style::Dimension::Points(val.height),
        }
    }
}

impl From<Size> for stretch::geometry::Size<f32> {
    fn from(val: Size) -> stretch::geometry::Size<f32> {
        stretch::geometry::Size {
            width: val.width,
            height: val.height,
        }
    }
}

impl From<stretch::geometry::Size<stretch::style::Dimension>> for Size {
    fn from(val: stretch::geometry::Size<stretch::style::Dimension>) -> Size {
        let width = match val.width {
            stretch::style::Dimension::Points(w) => w,
            _ => 0.0,
        };
        let height = match val.width {
            stretch::style::Dimension::Points(h) => h,
            _ => 0.0,
        };
        Size { width, height }
    }
}

/// State common to widgets and doms.
pub struct State<T> {
    pub style: Style,
    pub computed_style: Style,
    pub flex_node: Option<stretch::node::Node>,
    pub box_layout: BoxLayout,
    pub attrs: T,
    pub computed_attrs: T,
}

impl<T> Default for State<T>
where
    T: Default,
{
    fn default() -> State<T> {
        State {
            style: Style::default(),
            computed_style: Style::default(),
            flex_node: None,
            box_layout: BoxLayout::default(),
            attrs: T::default(),
            computed_attrs: T::default(),
        }
    }
}

impl<T> AsRef<Style> for State<T> {
    fn as_ref(&self) -> &Style {
        &self.style
    }
}

impl<T> AsMut<Style> for State<T> {
    fn as_mut(&mut self) -> &mut Style {
        &mut self.style
    }
}

impl<T> AsRef<BoxLayout> for State<T> {
    fn as_ref(&self) -> &BoxLayout {
        &self.box_layout
    }
}

impl<T> AsMut<BoxLayout> for State<T> {
    fn as_mut(&mut self) -> &mut BoxLayout {
        &mut self.box_layout
    }
}

impl<T> Deref for State<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.attrs
    }
}

impl<T> DerefMut for State<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.attrs
    }
}

impl<T> State<T> {
    pub fn resize(&mut self, offset: Location, scale_factor: f32)
    where
        T: Resize + fmt::Debug,
    {
        self.computed_style = self.style.resize(offset, scale_factor);
        self.computed_attrs = self.attrs.resize(offset, scale_factor);
    }

    pub fn as_computed_style(&self) -> &Style {
        &self.computed_style
    }

    pub fn as_computed_attrs(&self) -> &T {
        &self.computed_attrs
    }
}

#[derive(Clone, Copy, Default)]
pub struct BoxLayout {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl fmt::Display for BoxLayout {
    fn fmt(&self, f: &mut fmt::Formatter) -> result::Result<(), fmt::Error> {
        write!(f, "Box<{},{}..{},{}>", self.x, self.y, self.w, self.h)
    }
}

impl From<stretch::result::Layout> for BoxLayout {
    fn from(val: stretch::result::Layout) -> BoxLayout {
        let stretch::result::Layout {
            size: stretch::geometry::Size { width, height },
            location: stretch::geometry::Point { x, y },
            ..
        } = val;

        BoxLayout {
            x,
            y,
            w: width,
            h: height,
        }
    }
}

impl BoxLayout {
    pub fn to_aspect_ratio(&self) -> AspectRatio {
        if self.w > self.h {
            let x = 1.0;
            let y = self.h / self.w;
            AspectRatio((x, y).into())
        } else {
            let x = self.w / self.h;
            let y = 1.0;
            AspectRatio((x, y).into())
        }
    }

    pub fn to_ncc(&self, point: Point2<f32>) -> Point2<f32> {
        let ar = self.to_aspect_ratio();
        let x = (point.x / self.w) * ar.x;
        let y = (point.y / self.h) * ar.y;
        (x, y).into()
    }

    pub fn to_ndc(&self, point: Point2<f32>) -> Point2<f32> {
        let ar = self.to_aspect_ratio();
        let x = (point.x / self.w) / ar.x;
        let y = (point.y / self.h) / ar.y;
        (x, y).into()
    }

    pub fn to_viewport(&self) -> Viewport {
        Viewport {
            x: self.x,
            y: self.y,
            w: self.w,
            h: self.h,
            min_depth: 1.0,
            max_depth: 1.0,
        }
    }

    pub fn to_origin(&self) -> Point2<f32> {
        (self.x, self.y).into()
    }
}

pub struct AspectRatio(Point2<f32>);

impl Deref for AspectRatio {
    type Target = Point2<f32>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for AspectRatio {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct BoxVertex {
    pub position: [f32; 4],
}

impl BoxVertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![
        0 => Float32x4,
    ];

    pub fn to_vertex_buffer_layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        use std::mem;

        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<BoxVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub min_depth: f32,
    pub max_depth: f32,
}

impl Viewport {
    pub fn root_viewport(size: Size) -> Viewport {
        Viewport {
            x: 0.0,
            y: 0.0,
            w: size.width,
            h: size.height,
            min_depth: 1.0,
            max_depth: 1.0,
        }
    }

    pub fn set_viewport(&self, render_pass: &mut wgpu::RenderPass) {
        render_pass.set_viewport(
            self.x,
            self.y,
            self.w,
            self.h,
            self.min_depth,
            self.max_depth,
        );
    }
}
