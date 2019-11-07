extern crate quicksilver;
extern crate lru;

pub mod tile_world {
    use noise::{NoiseFn, HybridMulti};
    use std::collections::HashMap;
    use quicksilver::geom::Rectangle;
    use lru::LruCache;

    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct GridCoord {
        pub x: i64,
        pub y: i64
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub enum TileValue {
        Empty,
        Rock,
        Error
    }

    // Must be power of 2
    const PARTITION_SIZE: u8 = (1 << 4);

    pub struct AreaChanges {
        // TODO: Implement array mode for this structure for areas of dense change
        changes: HashMap<GridCoord, TileValue>
    }

    pub struct TileMap {
        pub rock_density: f64,
        pub selected_tile: GridCoord,
        generator_func: HybridMulti,
        // TODO add map changes data structure here
        // Concept: Since changes will likely concentrated in a few areas, but there may be small changes all over the map
        // Spatial partition by zeroing out the last ~4 bits of a position (16x16 groups) and then 
        // for sparse changes (a few mined rocks) - do a hash table to find any changes within those 256 tiles (sparse storage, slower but less memory used)
        // for dense areas (a base) - Keep a 2D array of all 256 tiles in this chunk (and save that whole thing)
        // The exact switchover point should be tuned over time, but things to consider while doing that are:
        //      - switch should be before the point where hash collisions start being likely
        //      - should delay as much as sensible, 2D array will be much bigger memory hog and not scale well
        //      - the main tiles that a players base is on should definitely be in the array once it expands, so looking at common bases seems like a good way to tune this
        // Game saving thoughts: 
        //      - Could also use this partitioning to not load whole save files on start up, load more lazily
        //      - Alternatively, could ignore the partitioning for the save files to make it easier to tweak things like sizes and internal behavior later (don't save 2d arrays just a bunch o changes)
        map_changes: HashMap<GridCoord, AreaChanges>,

        // TODO cache world sample queries
        // Re-generating untouched space and/or re-querying the changes data is expensive, so lets not do that every frame for every visible tile
        // Cache sizing still needs to be figured out - could be dynamic with camera size or just always big enough for max zoom
        tile_cache: LruCache<GridCoord, TileValue>
    }

    impl AreaChanges {
        fn new() -> AreaChanges {
            AreaChanges { changes: HashMap::new() }
        }

        fn sample(&self, pos: &GridCoord) -> Option<&TileValue> {
            // For now this just forwards the query to the internal hashmap
            // In future once a second storage type is available this will have to use the correct one
            self.changes.get(&pos)
        }

        fn add_change(&mut self, pos: &GridCoord, tile_value: &TileValue) {
            // Insert will overwrite old values with that key, so this is just always the correct option
            // TODO: add storage type switch here before insert
            self.changes.insert(pos.clone(), tile_value.clone());
        }
    }

    impl TileMap {
        pub fn new() -> TileMap {
            let generator_func = HybridMulti::new();
            TileMap { 
                generator_func, 
                rock_density: 0.5, 
                selected_tile: GridCoord{ x: 0, y: 0 }, 
                map_changes: HashMap::new(), 
                tile_cache: LruCache::new(256) 
            }
        }

        pub fn sample(&mut self, pos: &GridCoord) -> TileValue {
            // Unwrap values from struct
            let x = pos.x;
            let y = pos.y;

            // TODO: Check a cache before doing all the work of looking up the stored data
            let cache_result = self.tile_cache.get(pos);

            if cache_result.is_some() {
                return cache_result.unwrap().clone();
            }
            else {
                // Mask away the bits 
                let partition_x = x & !(PARTITION_SIZE as i64 - 1);
                let partition_y = y & !(PARTITION_SIZE as i64 - 1);
                let partition_coord = GridCoord { x: partition_x, y: partition_y };

                // Check the history for a matching change
                // First see if there is any changes within this tiles partition
                if self.map_changes.contains_key(&partition_coord)  {
                    // Ask the partition if there is a value for this tile
                    let tile_value: Option<&TileValue> = self.map_changes.get(&partition_coord).unwrap().sample(pos);
                    if tile_value.is_some() {
                        // There is a changed value in this tile, use that
                        return tile_value.unwrap().clone();
                    }
                }

                // If no edits have been applied to this tile, sample the noise function to decide what goes here
                // Noise is from -1..1 but I only want 0..1 so shift it first
                let value = ((self.generator_func.get([x as f64, y as f64]) + 1.0) / (2.0 + self.rock_density)).round();
                let value = if value > 1.0 { 1.0 } else if value < 0.0 { 0.0 } else { value };
                let tile_val = match value as i32 {
                    0 => TileValue::Empty,
                    1 => TileValue::Rock,
                    _ => TileValue::Error
                };

                self.tile_cache.put(*pos, tile_val);

                return tile_val;
            }
        }


        pub fn for_each_tile<F>(&mut self, bounds: &Rectangle, mut func: F)
            where F : FnMut(&GridCoord, &TileValue) {
            // TODO: Optimize the shit out of this
            /* Ideas for this: 
                - Don't need to resample noise very frame since most of the tiles are the same, only need to sample on the edges or when there is a change
                - Don't need to make a new transform every frame either, same reason
                - Use faster noise function
                - Is the color copy slow?
            */

            // Bounds to draw between
            let x_min = bounds.pos.x.floor() as i64;
            let x_max =(bounds.pos.x + bounds.size.x).ceil() as i64;
            let y_min = bounds.pos.y.floor() as i64;
            let y_max =(bounds.pos.y + bounds.size.y).ceil() as i64;
            
            // Draw one sprite rectangle for each tile within the bounds
            for x in x_min..x_max {
                for y in y_min..y_max {
                    let coord = GridCoord {x, y};
                    func(&coord, &self.sample(&coord));
                }
            }
        }

        pub fn pos_to_grid(&self, world_x: f32 , world_y: f32) -> GridCoord {
            GridCoord { x: world_x as i64, y: world_y as i64}
        }

        pub fn make_change(&mut self, pos: &GridCoord, new_value: &TileValue) {
            // Unwrap values from struct
            let x = pos.x;
            let y = pos.y;

            // Mask away the bits 
            let partition_x = x & !(PARTITION_SIZE as i64 - 1);
            let partition_y = y & !(PARTITION_SIZE as i64 - 1);
            let partition_coord = GridCoord { x: partition_x, y: partition_y };

            // First ensure there is a change table for this partition
            if !self.map_changes.contains_key(&partition_coord)  {
                self.map_changes.insert(partition_coord, AreaChanges::new());
            }

            // Safe to unwrap immediately because we know at this point the key is in the table
            let partition_changes = self.map_changes.get_mut(&partition_coord).unwrap();
            partition_changes.add_change(pos, new_value);

            // Put the new value in cache because presumably someone's going to want to know about this change
            self.tile_cache.put(*pos, *new_value);
        }

        // If new size is smaller, elements will be dropped
        pub fn resize_cache(&mut self, new_size: usize) {
            self.tile_cache.resize(new_size);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tile_world::{
        TileMap, TileValue, GridCoord
    };

    use quicksilver::{
        geom::{Rectangle},
    };

    fn is_valid_tile(value: &TileValue) -> bool {
        value == &TileValue::Empty || value == &TileValue::Rock
    }

    #[test]
    fn empty_map_access_gives_valid() {
        let mut map = TileMap::new();
        assert!(is_valid_tile(&map.sample(&GridCoord{x: 0, y: 0})));
    }

    #[test]
    fn untouched_map_no_errors() {
        let mut map = TileMap::new();
        
        // Check the 1 million tiles closest to origin
        let x_min: i64 = -500;
        let x_max: i64 = 500;
        let y_min: i64 = -500;
        let y_max: i64 = 500;

        for x in x_min..x_max {
            for y in y_min..y_max {
                assert!(is_valid_tile(&map.sample(&GridCoord{x, y})), "Found invalid tile at ({}, {})", x, y);
            }
        }
    }

    #[test]
    fn map_write_get_back_1() {
        let mut map = TileMap::new();
        map.make_change(&GridCoord{x: 0, y: 0}, &TileValue::Error);
        assert_eq!(map.sample(&GridCoord{x: 0, y: 0}), TileValue::Error);
    }

    #[test]
    fn map_write_get_back_dense_100() {
        let mut map = TileMap::new();

        // Large bounds but still within a single cache
        let x_min: i64 = 0;
        let x_max: i64 = 10;
        let y_min: i64 = 0;
        let y_max: i64 = 10;
        
        for x in x_min..x_max {
            for y in y_min..y_max {
                map.make_change(&GridCoord{x, y}, &TileValue::Error);
            }
        }

        for x in x_min..x_max {
            for y in y_min..y_max {
                assert_eq!(map.sample(&GridCoord{x, y}), TileValue::Error);
            }
        }
    }

    #[test]
    fn map_write_get_back_sparse_100() {
        let mut map = TileMap::new();

        // Large bounds but still within a single cache
        let x_min: i64 = 0;
        let x_max: i64 = 10;
        let y_min: i64 = 0;
        let y_max: i64 = 10;
        
        for x in x_min..x_max {
            for y in y_min..y_max {
                map.make_change(&GridCoord{x: 100 * x, y: 100 * y}, &TileValue::Error);
            }
        }

        for x in x_min..x_max {
            for y in y_min..y_max {
                assert_eq!(map.sample(&GridCoord{x: 100 * x, y: 100 * y}), TileValue::Error);
            }
        }
    }

    #[test]
    fn map_write_get_back_10000() {
        let mut map = TileMap::new();

        // Bounds big enough to be waaay beyond the cache
        let x_min: i64 = 0;
        let x_max: i64 = 100;
        let y_min: i64 = 0;
        let y_max: i64 = 100;
        
        for x in x_min..x_max {
            for y in y_min..y_max {
                map.make_change(&GridCoord{x, y}, &TileValue::Error);
            }
        }

        for x in x_min..x_max {
            for y in y_min..y_max {
                assert_eq!(map.sample(&GridCoord{x, y}), TileValue::Error);
            }
        }
    }

    #[test]
    fn pos_to_grid() {
        let map = TileMap::new();
        assert_eq!(map.pos_to_grid(0.0, 0.0), GridCoord{x: 0, y: 0});
        assert_eq!(map.pos_to_grid(0.1, 0.1), GridCoord{x: 0, y: 0});
        assert_eq!(map.pos_to_grid(-0.1, -0.1), GridCoord{x: 0, y: 0});
        assert_eq!(map.pos_to_grid(0.6, -0.6), GridCoord{x: 0, y: 0});
        assert_eq!(map.pos_to_grid(1.0, 1.0), GridCoord{x: 1, y: 1});
    }

    #[test]
    fn for_each_tile_bounds_gets_all() {
        let mut map = TileMap::new();
        // Create a rectangle from (0, 0) to (10, 10)
        let bounds = Rectangle::new_sized((10, 10));
        let mut tiles_hit: u32 = 0;

        let min_val = 0;
        let max_val = 10;

        map.for_each_tile(&bounds, |pos: &GridCoord, _value: &TileValue| {
            tiles_hit += 1;
            assert!(pos.x >= min_val, "Expected X greater than {}, got {}", min_val, pos.x);
            assert!(pos.x <= max_val, "Expected X less than {}, got {}", max_val, pos.x);
            assert!(pos.y >= min_val, "Expected Y greater than {}, got {}", min_val, pos.y);
            assert!(pos.y <= max_val, "Expected Y less than {}, got {}", max_val, pos.y);
        });

        assert_eq!(tiles_hit, 100);
    }
}
