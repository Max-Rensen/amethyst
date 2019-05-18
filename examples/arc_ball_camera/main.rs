//! Demonstrates how to use the fly camera

use amethyst::{
    assets::{PrefabLoader, PrefabLoaderSystem, RonFormat},
    controls::{ArcBallControlBundle, ArcBallControlTag},
    core::{
        shrev::{EventChannel, ReaderId},
        transform::{Transform, TransformBundle},
        Float,
    },
    ecs::prelude::{
        Join, Read, ReadExpect, ReadStorage, Resources, System, SystemData, WriteStorage,
    },
    input::{InputBundle, InputEvent, ScrollDirection, StringBindings},
    prelude::*,
    renderer::{
        camera::{Camera, Projection},
        formats::texture::ImageFormat,
        palette::{LinSrgba, Srgb, Srgba},
        pass::{DrawShadedDesc, DrawSkyboxDesc},
        rendy::{
            factory::Factory,
            graph::{
                render::{RenderGroupDesc, SubpassBuilder},
                GraphBuilder,
            },
            hal::format::Format,
            mesh::{Normal, Position, Tangent, TexCoord},
        },
        transparent::Transparent,
        types::DefaultBackend,
        GraphCreator, RenderingSystem, Texture,
    },
    utils::{application_root_dir, scene::BasicScenePrefab},
    window::{ScreenDimensions, Window, WindowBundle},
    Error,
};
use std::hash::Hash;
use std::sync::Arc;

type MyPrefabData = BasicScenePrefab<(Vec<Position>, Vec<Normal>, Vec<TexCoord>), f32>;

struct ExampleState;

impl SimpleState for ExampleState {
    fn on_start(&mut self, data: StateData<'_, GameData<'_, '_>>) {
        let prefab_handle = data.world.exec(|loader: PrefabLoader<'_, MyPrefabData>| {
            loader.load("prefab/arc_ball_camera.ron", RonFormat, ())
        });
        data.world.create_entity().with(prefab_handle).build();
    }
}

struct CameraDistanceSystem<AC>
where
    AC: Hash + Eq + 'static,
{
    event_reader: Option<ReaderId<InputEvent<AC>>>,
}

impl<AC> CameraDistanceSystem<AC>
where
    AC: Hash + Eq + 'static,
{
    pub fn new() -> Self {
        CameraDistanceSystem { event_reader: None }
    }
}

impl<'a, AC> System<'a> for CameraDistanceSystem<AC>
where
    AC: Hash + Eq + Clone + Send + Sync + 'static,
{
    type SystemData = (
        Read<'a, EventChannel<InputEvent<AC>>>,
        ReadStorage<'a, Transform>,
        WriteStorage<'a, ArcBallControlTag>,
    );

    fn run(&mut self, (events, transforms, mut tags): Self::SystemData) {
        for event in events.read(&mut self.event_reader.as_mut().unwrap()) {
            match *event {
                InputEvent::MouseWheelMoved(direction) => match direction {
                    ScrollDirection::ScrollUp => {
                        for (_, tag) in (&transforms, &mut tags).join() {
                            tag.distance *= Float::from(0.9);
                        }
                    }
                    ScrollDirection::ScrollDown => {
                        for (_, tag) in (&transforms, &mut tags).join() {
                            tag.distance *= Float::from(1.1);
                        }
                    }
                    _ => (),
                },
                _ => (),
            }
        }
    }

    fn setup(&mut self, res: &mut Resources) {
        Self::SystemData::setup(res);

        self.event_reader = Some(
            res.fetch_mut::<EventChannel<InputEvent<AC>>>()
                .register_reader(),
        );
    }
}

fn main() -> Result<(), Error> {
    amethyst::start_logger(Default::default());

    let app_root = application_root_dir()?;

    let resources_directory = app_root.join("examples/assets");
    let display_config_path =
        app_root.join("examples/arc_ball_camera/resources/display_config.ron");

    let key_bindings_path = app_root.join("examples/arc_ball_camera/resources/input.ron");

    let game_data = GameDataBuilder::default()
        .with(PrefabLoaderSystem::<MyPrefabData>::default(), "", &[])
        .with_bundle(TransformBundle::new().with_dep(&[]))?
        .with_bundle(
            InputBundle::<StringBindings>::new().with_bindings_from_file(&key_bindings_path)?,
        )?
        .with_bundle(ArcBallControlBundle::<StringBindings>::new())?
        .with(
            CameraDistanceSystem::<String>::new(),
            "camera_distance_system",
            &["input_system"],
        )
        .with_bundle(WindowBundle::from_config_path(display_config_path))?
        .with_thread_local(RenderingSystem::<DefaultBackend, _>::new(
            ExampleGraph::new(),
        ));
    let mut game = Application::build(resources_directory, ExampleState)?.build(game_data)?;
    game.run();
    Ok(())
}

struct ExampleGraph {
    last_dimensions: Option<ScreenDimensions>,
    surface_format: Option<Format>,
    dirty: bool,
}

impl ExampleGraph {
    pub fn new() -> Self {
        Self {
            last_dimensions: None,
            surface_format: None,
            dirty: true,
        }
    }
}

impl GraphCreator<DefaultBackend> for ExampleGraph {
    fn rebuild(&mut self, res: &Resources) -> bool {
        // Rebuild when dimensions change, but wait until at least two frames have the same.
        let new_dimensions = res.try_fetch::<ScreenDimensions>();
        use std::ops::Deref;
        if self.last_dimensions.as_ref() != new_dimensions.as_ref().map(|d| d.deref()) {
            self.dirty = true;
            self.last_dimensions = new_dimensions.map(|d| d.clone());
            return false;
        }
        return self.dirty;
    }

    fn builder(
        &mut self,
        factory: &mut Factory<DefaultBackend>,
        res: &Resources,
    ) -> GraphBuilder<DefaultBackend, Resources> {
        use amethyst::renderer::rendy::{
            graph::present::PresentNode,
            hal::command::{ClearDepthStencil, ClearValue},
        };

        self.dirty = false;

        let window = <ReadExpect<'_, Arc<Window>>>::fetch(res);
        let surface = factory.create_surface(window.clone());
        // cache surface format to speed things up
        let surface_format = *self
            .surface_format
            .get_or_insert_with(|| factory.get_surface_format(&surface));

        let mut graph_builder = GraphBuilder::new();
        let color = graph_builder.create_image(
            surface.kind(),
            1,
            surface_format,
            Some(ClearValue::Color([0.34, 0.36, 0.52, 1.0].into())),
        );

        let depth = graph_builder.create_image(
            surface.kind(),
            1,
            Format::D32Sfloat,
            Some(ClearValue::DepthStencil(ClearDepthStencil(1.0, 0))),
        );

        let opaque = graph_builder.add_node(
            SubpassBuilder::new()
                .with_group(DrawShadedDesc::default().builder())
                .with_group(
                    DrawSkyboxDesc::with_colors(
                        Srgb::new(0.82, 0.51, 0.50),
                        Srgb::new(0.18, 0.11, 0.85),
                    )
                    .builder(),
                )
                .with_color(color)
                .with_depth_stencil(depth)
                .into_pass(),
        );

        let _present = graph_builder
            .add_node(PresentNode::builder(factory, surface, color).with_dependency(opaque));

        graph_builder
    }
}
