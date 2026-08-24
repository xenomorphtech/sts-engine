use crate::ids::RoomType;
use crate::rng::StsRandom;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapEdge {
    pub src_x: i32,
    pub src_y: i32,
    pub dst_x: i32,
    pub dst_y: i32,
    pub taken: bool,
}

impl PartialOrd for MapEdge {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MapEdge {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.dst_x
            .cmp(&other.dst_x)
            .then(self.dst_y.cmp(&other.dst_y))
    }
}

#[derive(Clone, Debug)]
pub struct MapNode {
    pub x: i32,
    pub y: i32,
    pub room: Option<RoomType>,
    pub taken: bool,
    pub emerald_key: bool,
    pub edges: Vec<MapEdge>,
    pub parents: Vec<(i32, i32)>,
}

impl MapNode {
    fn new(x: i32, y: i32) -> Self {
        Self {
            x,
            y,
            room: None,
            taken: false,
            emerald_key: false,
            edges: Vec::new(),
            parents: Vec::new(),
        }
    }

    pub fn has_edges(&self) -> bool {
        !self.edges.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct DungeonMap {
    pub nodes: Vec<Vec<MapNode>>,
}

impl DungeonMap {
    pub fn height(&self) -> usize {
        self.nodes.len()
    }

    pub fn width(&self) -> usize {
        self.nodes.first().map(|r| r.len()).unwrap_or(0)
    }

    pub fn node(&self, x: i32, y: i32) -> &MapNode {
        &self.nodes[y as usize][x as usize]
    }

    pub fn node_mut(&mut self, x: i32, y: i32) -> &mut MapNode {
        &mut self.nodes[y as usize][x as usize]
    }
}

pub const MAP_HEIGHT: i32 = 15;
pub const MAP_WIDTH: i32 = 7;
pub const MAP_DENSITY: i32 = 6;

pub fn generate_dungeon(
    height: i32,
    width: i32,
    path_density: i32,
    rng: &mut StsRandom,
) -> DungeonMap {
    let mut map = create_nodes(height, width);
    create_paths(&mut map, path_density, rng);
    filter_redundant_edges_from_row(&mut map);
    map
}

fn create_nodes(height: i32, width: i32) -> DungeonMap {
    let mut nodes = Vec::with_capacity(height as usize);
    for y in 0..height {
        let mut row = Vec::with_capacity(width as usize);
        for x in 0..width {
            row.push(MapNode::new(x, y));
        }
        nodes.push(row);
    }
    DungeonMap { nodes }
}

fn rand_range(rng: &mut StsRandom, min: i32, max: i32) -> i32 {
    rng.random_int(max - min) + min
}

fn create_paths(map: &mut DungeonMap, path_density: i32, rng: &mut StsRandom) {
    let row_size = map.width() as i32 - 1;
    let mut first_starting_node = -1;
    for i in 0..path_density {
        let mut starting_node = rand_range(rng, 0, row_size);
        if i == 0 {
            first_starting_node = starting_node;
        }
        while starting_node == first_starting_node && i == 1 {
            starting_node = rand_range(rng, 0, row_size);
        }
        create_path(
            map,
            MapEdge {
                src_x: starting_node,
                src_y: -1,
                dst_x: starting_node,
                dst_y: 0,
                taken: false,
            },
            rng,
        );
    }
}

fn create_path(map: &mut DungeonMap, edge: MapEdge, rng: &mut StsRandom) {
    let height = map.height() as i32;
    if edge.dst_y + 1 >= height {
        let current = map.node_mut(edge.dst_x, edge.dst_y);
        current.edges.push(MapEdge {
            src_x: edge.dst_x,
            src_y: edge.dst_y,
            dst_x: 3,
            dst_y: edge.dst_y + 2,
            taken: false,
        });
        current.edges.sort();
        return;
    }

    let row_end_node = map.width() as i32 - 1;
    let (min, max) = if edge.dst_x == 0 {
        (0, 1)
    } else if edge.dst_x == row_end_node {
        (-1, 0)
    } else {
        (-1, 1)
    };

    let mut new_edge_x = edge.dst_x + rand_range(rng, min, max);
    let new_edge_y = edge.dst_y + 1;
    let current_x = edge.dst_x;
    let current_y = edge.dst_y;

    let parents = map.node(new_edge_x, new_edge_y).parents.clone();
    if !parents.is_empty() {
        for parent in &parents {
            if *parent != (current_x, current_y) {
                if let Some(ancestor) = common_ancestor(map, *parent, (current_x, current_y), 5) {
                    let ancestor_gap = new_edge_y - ancestor.1;
                    if ancestor_gap < 3 {
                        let target_x = map.node(new_edge_x, new_edge_y).x;
                        if target_x > current_x {
                            new_edge_x = edge.dst_x + rand_range(rng, -1, 0);
                            if new_edge_x < 0 {
                                new_edge_x = edge.dst_x;
                            }
                        } else if target_x == current_x {
                            new_edge_x = edge.dst_x + rand_range(rng, -1, 1);
                            if new_edge_x > row_end_node {
                                new_edge_x = edge.dst_x - 1;
                            } else if new_edge_x < 0 {
                                new_edge_x = edge.dst_x + 1;
                            }
                        } else {
                            new_edge_x = edge.dst_x + rand_range(rng, 0, 1);
                            if new_edge_x > row_end_node {
                                new_edge_x = edge.dst_x;
                            }
                        }
                    }
                }
            }
        }
    }

    if edge.dst_x != 0 {
        let left = map.node(edge.dst_x - 1, edge.dst_y);
        if left.has_edges() {
            let right_edge = left.edges.iter().max().unwrap();
            if right_edge.dst_x > new_edge_x {
                new_edge_x = right_edge.dst_x;
            }
        }
    }
    if edge.dst_x < row_end_node {
        let right = map.node(edge.dst_x + 1, edge.dst_y);
        if right.has_edges() {
            let left_edge = right.edges.iter().min().unwrap();
            if left_edge.dst_x < new_edge_x {
                new_edge_x = left_edge.dst_x;
            }
        }
    }

    {
        let current = map.node_mut(current_x, current_y);
        current.edges.push(MapEdge {
            src_x: current_x,
            src_y: current_y,
            dst_x: new_edge_x,
            dst_y: new_edge_y,
            taken: false,
        });
        current.edges.sort();
    }
    map.node_mut(new_edge_x, new_edge_y)
        .parents
        .push((current_x, current_y));
    create_path(
        map,
        MapEdge {
            src_x: current_x,
            src_y: current_y,
            dst_x: new_edge_x,
            dst_y: new_edge_y,
            taken: false,
        },
        rng,
    );
}

/// Intentionally copies the Java bug `node1.x < node2.y`.
fn common_ancestor(
    map: &DungeonMap,
    node1: (i32, i32),
    node2: (i32, i32),
    max_depth: i32,
) -> Option<(i32, i32)> {
    let (mut l, mut r) = if node1.0 < node2.1 {
        (node1, node2)
    } else {
        (node2, node1)
    };
    let start_y = node1.1;
    let mut current_y = start_y;
    while current_y >= 0 && current_y >= start_y - max_depth {
        let l_parents = &map.node(l.0, l.1).parents;
        let r_parents = &map.node(r.0, r.1).parents;
        if l_parents.is_empty() || r_parents.is_empty() {
            return None;
        }
        l = *l_parents.iter().max_by_key(|p| p.0).unwrap();
        r = *r_parents.iter().min_by_key(|p| p.0).unwrap();
        if l == r {
            return Some(l);
        }
        current_y -= 1;
    }
    None
}

fn filter_redundant_edges_from_row(map: &mut DungeonMap) {
    let mut existing = Vec::new();
    let width = map.width();
    for x in 0..width {
        let mut delete_idx = Vec::new();
        {
            let node = &map.nodes[0][x];
            if !node.has_edges() {
                continue;
            }
            for (i, edge) in node.edges.iter().enumerate() {
                if existing
                    .iter()
                    .any(|prev: &MapEdge| prev.dst_x == edge.dst_x && prev.dst_y == edge.dst_y)
                {
                    delete_idx.push(i);
                }
                existing.push(edge.clone());
            }
        }
        let node = &mut map.nodes[0][x];
        for i in delete_idx.into_iter().rev() {
            node.edges.remove(i);
        }
    }
}

pub fn assign_row(map: &mut DungeonMap, y: usize, room: RoomType) {
    for node in &mut map.nodes[y] {
        if node.room.is_none() {
            node.room = Some(room);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomKind {
    Monster,
    Elite,
    Event,
    Rest,
    Shop,
    Treasure,
}

impl RoomKind {
    pub fn to_type(self) -> RoomType {
        match self {
            RoomKind::Monster => RoomType::Monster,
            RoomKind::Elite => RoomType::Elite,
            RoomKind::Event => RoomType::Event,
            RoomKind::Rest => RoomType::Rest,
            RoomKind::Shop => RoomType::Shop,
            RoomKind::Treasure => RoomType::Treasure,
        }
    }
}

/// The Ending is a 5-row, 7-col grid with a single path: rest → shop → Shield and Spear → Heart.
pub fn generate_ending_map() -> DungeonMap {
    let mut nodes = Vec::new();
    for y in 0..5 {
        let mut row = Vec::new();
        for x in 0..MAP_WIDTH {
            row.push(MapNode::new(x, y));
        }
        nodes.push(row);
    }
    let mut map = DungeonMap { nodes };
    let rooms = [
        (3, 0, RoomType::Rest),
        (3, 1, RoomType::Shop),
        (3, 2, RoomType::Elite),
        (3, 3, RoomType::Boss),
    ];
    for &(x, y, room) in &rooms {
        map.node_mut(x, y).room = Some(room);
    }
    for &(x, y, _) in &rooms[..rooms.len() - 1] {
        let (dx, dy) = (x, y + 1);
        map.node_mut(x, y).edges.push(MapEdge {
            src_x: x,
            src_y: y,
            dst_x: dx,
            dst_y: dy,
            taken: false,
        });
        map.node_mut(dx, dy).parents.push((x, y));
    }
    map
}

pub fn distribute_rooms(map: &mut DungeonMap, rng: &mut StsRandom, mut rooms: Vec<RoomKind>) {
    let node_count = connected_unassigned(map);
    while rooms.len() < node_count {
        rooms.push(RoomKind::Monster);
    }
    crate::java_util::shuffle_xs128(&mut rooms, &mut rng.random);
    assign_rooms_to_nodes(map, &mut rooms);
    last_minute_fill(map);
}

fn connected_unassigned(map: &DungeonMap) -> usize {
    map.nodes
        .iter()
        .flatten()
        .filter(|n| n.has_edges() && n.room.is_none())
        .count()
}

fn assign_rooms_to_nodes(map: &mut DungeonMap, rooms: &mut Vec<RoomKind>) {
    let height = map.height();
    let width = map.width();
    for y in 0..height {
        for x in 0..width {
            if map.nodes[y][x].has_edges() && map.nodes[y][x].room.is_none() {
                if let Some(idx) = next_room_index(map, x, y, rooms) {
                    let kind = rooms.remove(idx);
                    map.nodes[y][x].room = Some(kind.to_type());
                }
            }
        }
    }
}

fn next_room_index(map: &DungeonMap, x: usize, y: usize, rooms: &[RoomKind]) -> Option<usize> {
    let parents = map.nodes[y][x].parents.clone();
    let siblings = siblings(map, &parents, x as i32, y as i32);
    for (i, room) in rooms.iter().enumerate() {
        if rule_assignable(y as i32, *room)
            && (!rule_parent_matches(map, &parents, *room)
                && !rule_sibling_matches(map, &siblings, *room)
                || y == 0)
        {
            return Some(i);
        }
    }
    None
}

fn siblings(map: &DungeonMap, parents: &[(i32, i32)], x: i32, y: i32) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for (px, py) in parents {
        for edge in &map.node(*px, *py).edges {
            if edge.dst_x != x || edge.dst_y != y {
                out.push((edge.dst_x, edge.dst_y));
            }
        }
    }
    out
}

fn rule_assignable(y: i32, room: RoomKind) -> bool {
    if y <= 4 && matches!(room, RoomKind::Rest | RoomKind::Elite) {
        return false;
    }
    if y >= 13 && matches!(room, RoomKind::Rest) {
        return false;
    }
    true
}

fn rule_parent_matches(map: &DungeonMap, parents: &[(i32, i32)], room: RoomKind) -> bool {
    if !matches!(
        room,
        RoomKind::Rest | RoomKind::Treasure | RoomKind::Shop | RoomKind::Elite
    ) {
        return false;
    }
    for (x, y) in parents {
        if map.node(*x, *y).room == Some(room.to_type()) {
            return true;
        }
    }
    false
}

fn rule_sibling_matches(map: &DungeonMap, siblings: &[(i32, i32)], room: RoomKind) -> bool {
    if !matches!(
        room,
        RoomKind::Rest | RoomKind::Monster | RoomKind::Event | RoomKind::Elite | RoomKind::Shop
    ) {
        return false;
    }
    for (x, y) in siblings {
        if map.node(*x, *y).room == Some(room.to_type()) {
            return true;
        }
    }
    false
}

fn last_minute_fill(map: &mut DungeonMap) {
    for row in &mut map.nodes {
        for node in row {
            if node.has_edges() && node.room.is_none() {
                node.room = Some(RoomType::Monster);
            }
        }
    }
}

pub fn java_round(value: f32) -> i32 {
    (value + 0.5).floor() as i32
}

pub fn generate_room_types(
    available: i32,
    shop_chance: f32,
    rest_chance: f32,
    elite_chance: f32,
    event_chance: f32,
    ascension: i32,
) -> Vec<RoomKind> {
    let shop = java_round(available as f32 * shop_chance);
    let rest = java_round(available as f32 * rest_chance);
    let elite = if ascension >= 1 {
        java_round(available as f32 * elite_chance * 1.6)
    } else {
        java_round(available as f32 * elite_chance)
    };
    let event = java_round(available as f32 * event_chance);
    let mut rooms = Vec::new();
    rooms.extend(std::iter::repeat_n(RoomKind::Shop, shop.max(0) as usize));
    rooms.extend(std::iter::repeat_n(RoomKind::Rest, rest.max(0) as usize));
    rooms.extend(std::iter::repeat_n(RoomKind::Elite, elite.max(0) as usize));
    rooms.extend(std::iter::repeat_n(RoomKind::Event, event.max(0) as usize));
    rooms
}
