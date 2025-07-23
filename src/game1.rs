use bevy::prelude::*;
use bevy::render::view::RenderLayers;
use rand::prelude::*;

#[derive(Event)]
pub struct CommandEvent {
    pub unit_id: Entity,
    pub command: CommandType,
}

pub enum CommandType {
    MoveTo(Vec2),
}

pub struct Game1Plugin;

impl Plugin for Game1Plugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SelectionArea::default())
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (
                    selection_area_start,
                    selection_area_update,
                    selection_area_finish,
                    draw_selection_area,
                    update_unit_colors,
                    move_selected_units,
                    unit_command_system,
                    move_units_system,
                    select_unit_on_click,
                ),
            );
    }
}

#[derive(Component)]
pub struct UnitTag;

#[derive(Component, Default)]
pub struct Selected;

#[derive(Component)]
struct SelectionRect;

#[derive(Default, Resource)]
pub struct SelectionArea {
    pub start: Option<Vec2>,
    pub end: Option<Vec2>,
}

#[derive(Component, Default)]
pub struct MoveTarget {
    pub position: Option<Vec2>,
}

const UNIT_COL: Color = Color::GREEN;
const SELECTED_COL: Color = Color::rgb(0.8, 0.2, 0.2);

fn setup(mut commands: Commands) {
    commands.spawn(Camera2dBundle::default());

    let spacing = 70.;
    for x in 0..4 {
        for y in 0..3 {
            commands.spawn((
                SpriteBundle {
                    sprite: Sprite {
                        color: UNIT_COL,
                        custom_size: Some(Vec2::splat(40.0)),
                        ..default()
                    },
                    transform: Transform::from_xyz(
                        x as f32 * spacing - 100.,
                        y as f32 * spacing - 60.,
                        0.,
                    ),
                    ..default()
                },
                UnitTag,
                MoveTarget::default(),
            ));
        }
    }

    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color: Color::rgba(0.1, 0.5, 1.0, 0.20),
                custom_size: Some(Vec2::ZERO),
                ..default()
            },
            transform: Transform::from_xyz(0., 0., 10.),
            visibility: Visibility::Hidden,
            ..default()
        },
        SelectionRect,
    ));
}

fn mouse_pos_to_world(cursor_pos: Vec2, window_size: Vec2) -> Vec2 {
    Vec2::new(cursor_pos.x, window_size.y - cursor_pos.y) - window_size / 2.
}

fn selection_area_start(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut area: ResMut<SelectionArea>,
) {
    if mouse.just_pressed(MouseButton::Left) {
        if let Some(pos) = windows.single().cursor_position() {
            area.start = Some(pos);
            area.end = Some(pos);
        }
    }
}

fn selection_area_update(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut area: ResMut<SelectionArea>,
) {
    if mouse.pressed(MouseButton::Left) {
        if let Some(pos) = windows.single().cursor_position() {
            area.end = Some(pos);
        }
    }
}

fn selection_area_finish(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut area: ResMut<SelectionArea>,
    mut q_units: Query<(&GlobalTransform, Entity, Option<&Selected>), With<UnitTag>>,
    mut commands: Commands,
) {
    if mouse.just_released(MouseButton::Left) {
        if let (Some(start), Some(end)) = (area.start, area.end) {
            if (start - end).length() > 5.0 {
                let window = windows.single();
                let win_size = Vec2::new(window.width(), window.height());

                let min_world = mouse_pos_to_world(start.min(end), win_size);
                let max_world = mouse_pos_to_world(start.max(end), win_size);

                let mins = Vec2::new(min_world.x.min(max_world.x), min_world.y.min(max_world.y));
                let maxs = Vec2::new(min_world.x.max(max_world.x), min_world.y.max(max_world.y));

                for (tr, entity, selected) in q_units.iter_mut() {
                    let pos = tr.translation().truncate();
                    if pos.x >= mins.x && pos.x <= maxs.x && pos.y >= mins.y && pos.y <= maxs.y {
                        if selected.is_none() {
                            commands.entity(entity).insert(Selected);
                        }
                    } else {
                        if selected.is_some() {
                            commands.entity(entity).remove::<Selected>();
                        }
                    }
                }
            }
        }
        area.start = None;
        area.end = None;
    }
}

fn draw_selection_area(
    area: Res<SelectionArea>,
    mut q: Query<(&mut Sprite, &mut Transform, &mut Visibility), With<SelectionRect>>,
    windows: Query<&Window>,
) {
    let (mut sprite, mut tr, mut vis) = q.single_mut();

    if let (Some(start), Some(end)) = (area.start, area.end) {
        let window = windows.single();
        let win_size = Vec2::new(window.width(), window.height());

        let world_start = mouse_pos_to_world(start, win_size);
        let world_end = mouse_pos_to_world(end, win_size);

        let center = (world_start + world_end) / 2.0;
        let size = (world_end - world_start).abs();

        sprite.custom_size = Some(size);
        tr.translation = center.extend(10.0);
        *vis = Visibility::Visible;
    } else {
        *vis = Visibility::Hidden;
    }
}

fn update_unit_colors(mut q: Query<(&mut Sprite, Option<&Selected>), With<UnitTag>>) {
    for (mut sprite, selected) in q.iter_mut() {
        if selected.is_some() {
            sprite.color = SELECTED_COL;
        } else {
            sprite.color = UNIT_COL;
        }
    }
}

fn move_selected_units(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut q_selected: Query<Entity, (With<UnitTag>, With<Selected>)>,
    mut ev_writer: EventWriter<CommandEvent>,
) {
    if mouse.just_pressed(MouseButton::Right) {
        let window = windows.single();
        if let Some(pos) = window.cursor_position() {
            let win_size = Vec2::new(window.width(), window.height());
            let base_pos = Vec2::new(pos.x, win_size.y - pos.y) - win_size / 2.;

            let mut rng = thread_rng();
            for entity in q_selected.iter_mut() {
                let offset_x: f32 = rng.gen_range(-15.0..15.0);
                let offset_y: f32 = rng.gen_range(-15.0..15.0);
                let target_pos = base_pos + Vec2::new(offset_x, offset_y);
                ev_writer.send(CommandEvent {
                    unit_id: entity,
                    command: CommandType::MoveTo(target_pos),
                });
            }
        }
    }
}

fn unit_command_system(
    mut commands: Commands,
    mut events: EventReader<CommandEvent>,
    mut query: Query<(Entity, &mut MoveTarget)>,
) {
    for event in events.read() {
        if let Ok((entity, mut move_target)) = query.get_mut(event.unit_id) {
            if let CommandType::MoveTo(pos) = &event.command {
                move_target.position = Some(*pos);
            }
        }
    }
}

fn select_unit_on_click(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut q_units: Query<(Entity, &GlobalTransform, &Sprite), With<UnitTag>>,
    mut commands: Commands,
    q_selected: Query<Entity, With<Selected>>,
) {
    if mouse.just_pressed(MouseButton::Left) {
        let window = windows.single();
        if let Some(cursor_pos) = window.cursor_position() {
            let win_size = Vec2::new(window.width(), window.height());
            let world_cursor_pos =
                Vec2::new(cursor_pos.x, win_size.y - cursor_pos.y) - win_size / 2.;
            for (entity, global_transform, sprite) in q_units.iter_mut() {
                let pos = global_transform.translation().truncate();
                let half_size = sprite.custom_size.unwrap_or(Vec2::ZERO) / 2.0;
                let min = pos - half_size;
                let max = pos + half_size;
                if world_cursor_pos.x >= min.x
                    && world_cursor_pos.x <= max.x
                    && world_cursor_pos.y >= min.y
                    && world_cursor_pos.y <= max.y
                {
                    for selected_entity in q_selected.iter() {
                        commands.entity(selected_entity).remove::<Selected>();
                    }
                    commands.entity(entity).insert(Selected);
                    return;
                }
            }

            // Клик не по юниту — снимаем выделение
            for selected_entity in q_selected.iter() {
                commands.entity(selected_entity).remove::<Selected>();
            }
        }
    }
}

fn move_units_system(mut query: Query<(&mut Transform, &mut MoveTarget)>, time: Res<Time>) {
    let speed = 150.0; // скорость в единицах в секунду

    for (mut transform, mut move_target) in query.iter_mut() {
        if let Some(target_pos) = move_target.position {
            let current_pos = transform.translation.truncate();
            let dir = target_pos - current_pos;
            let distance = dir.length();

            if distance < 1.0 {
                // Достигли цели
                move_target.position = None;
            } else {
                let movement = dir.normalize() * speed * time.delta_seconds();
                if movement.length() > distance {
                    transform.translation.x = target_pos.x;
                    transform.translation.y = target_pos.y;
                    move_target.position = None;
                } else {
                    transform.translation.x += movement.x;
                    transform.translation.y += movement.y;
                }
            }
        }
    }
}
