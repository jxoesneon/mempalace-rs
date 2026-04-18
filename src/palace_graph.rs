use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Default)]
pub struct PalaceGraph {
    /// Room name -> Set of wings it belongs to
    pub rooms: HashMap<String, HashSet<String>>,
    /// Wing name -> Set of rooms it contains
    pub wings: HashMap<String, HashSet<String>>,
}

impl PalaceGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reload taxonomy (wings and rooms) from the persistent memory store.
    pub fn load_from_db(&mut self, conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
        let mut stmt = conn.prepare("SELECT DISTINCT wing, room FROM memories")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        for row in rows {
            let (wing, room) = row?;
            self.add_room(&room, &wing);
        }
        Ok(())
    }

    pub fn add_room(&mut self, room: &str, wing: &str) {
        self.rooms
            .entry(room.to_string())
            .or_default()
            .insert(wing.to_string());
        self.wings
            .entry(wing.to_string())
            .or_default()
            .insert(room.to_string());
    }

    pub fn find_connected_rooms(&self, start_room: &str, max_hops: usize) -> Vec<String> {
        let start_node = match self.fuzzy_lookup(start_room) {
            Some(node) => node,
            None => return vec![],
        };

        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut results = Vec::new();

        visited.insert(start_node.clone());
        queue.push_back((start_node, 0));

        while let Some((current_room, hops)) = queue.pop_front() {
            if hops > 0 {
                results.push(current_room.clone());
            }

            if hops < max_hops {
                if let Some(wings) = self.rooms.get(&current_room) {
                    for wing in wings {
                        if let Some(rooms_in_wing) = self.wings.get(wing) {
                            for neighbor in rooms_in_wing {
                                if !visited.contains(neighbor) {
                                    visited.insert(neighbor.clone());
                                    queue.push_back((neighbor.clone(), hops + 1));
                                }
                            }
                        }
                    }
                }
            }
        }

        results.sort();
        results
    }

    pub fn find_tunnels(&self) -> Vec<String> {
        let mut tunnels: Vec<String> = self
            .rooms
            .iter()
            .filter(|(_, wings)| wings.len() > 1)
            .map(|(room, _)| room.clone())
            .collect();
        tunnels.sort();
        tunnels
    }

    pub fn fuzzy_lookup(&self, room_name: &str) -> Option<String> {
        if self.rooms.contains_key(room_name) {
            return Some(room_name.to_string());
        }

        let room_name_lower = room_name.to_lowercase();
        let mut best_match = None;
        let mut min_distance = usize::MAX;

        for existing_name in self.rooms.keys() {
            let existing_lower = existing_name.to_lowercase();
            if existing_lower == room_name_lower {
                return Some(existing_name.clone());
            }

            let dist = self.levenshtein(&room_name_lower, &existing_lower);
            if dist < min_distance && dist <= 2 {
                min_distance = dist;
                best_match = Some(existing_name.clone());
            }
        }

        best_match
    }

    fn levenshtein(&self, s1: &str, s2: &str) -> usize {
        let v1: Vec<char> = s1.chars().collect();
        let v2: Vec<char> = s2.chars().collect();
        let m = v1.len();
        let n = v2.len();
        let mut dp = vec![vec![0; n + 1]; m + 1];

        for (i, row) in dp.iter_mut().enumerate().take(m + 1) {
            row[0] = i;
        }
        for (j, val) in dp[0].iter_mut().enumerate() {
            *val = j;
        }

        for i in 1..=m {
            for j in 1..=n {
                if v1[i - 1] == v2[j - 1] {
                    dp[i][j] = dp[i - 1][j - 1];
                } else {
                    dp[i][j] = 1 + std::cmp::min(
                        dp[i - 1][j - 1],
                        std::cmp::min(dp[i - 1][j], dp[i][j - 1]),
                    );
                }
            }
        }
        dp[m][n]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_traversal() {
        let mut graph = PalaceGraph::new();
        graph.add_room("Kitchen", "Main");
        graph.add_room("Living Room", "Main");
        graph.add_room("Living Room", "West Wing");
        graph.add_room("Bedroom", "West Wing");

        let connected = graph.find_connected_rooms("Kitchen", 1);
        assert_eq!(connected, vec!["Living Room"]);

        let connected_2 = graph.find_connected_rooms("Kitchen", 2);
        let mut expected = vec!["Bedroom", "Living Room"];
        expected.sort();
        let mut actual = connected_2.clone();
        actual.sort();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_tunnel_detection() {
        let mut graph = PalaceGraph::new();
        graph.add_room("Kitchen", "Main");
        graph.add_room("Living Room", "Main");
        graph.add_room("Living Room", "West Wing");
        graph.add_room("Bedroom", "West Wing");

        let tunnels = graph.find_tunnels();
        assert_eq!(tunnels, vec!["Living Room"]);
    }

    #[test]
    fn test_fuzzy_lookup() {
        let mut graph = PalaceGraph::new();
        graph.add_room("Kitchen", "Main");

        assert_eq!(graph.fuzzy_lookup("Kitchin"), Some("Kitchen".to_string()));
        assert_eq!(graph.fuzzy_lookup("kitchen"), Some("Kitchen".to_string()));
        assert_eq!(graph.fuzzy_lookup("Unknown"), None);
    }
}
