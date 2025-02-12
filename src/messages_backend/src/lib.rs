use std::{borrow::Cow, cell::RefCell};
use ic_stable_structures::{memory_manager::{MemoryId, MemoryManager, VirtualMemory},DefaultMemoryImpl , StableBTreeMap,};
use candid::{CandidType, Decode, Encode, Principal};
use ic_stable_structures::{storable::Bound, Storable};
use serde::Deserialize;
use ic_cdk::{query, update};

// Define the Post struct
#[derive(CandidType, Deserialize, Debug, Clone)]
pub struct Post {
    id: u64,
    content: String,
}

type Memory = VirtualMemory<DefaultMemoryImpl>;

impl Storable for Post {
    const BOUND: ic_stable_structures::storable::Bound = Bound::Unbounded;

    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(&bytes.as_ref(), Self).unwrap_or_else(|e| {
            ic_cdk::api::print(&format!("Failed to decode Post: {:?}", e));
            Post {
                id: 0,
                content: String::from(""),
            }
        })
    }
}

// Define the UserPosts struct to hold a list of post IDs
#[derive(CandidType, Deserialize, Debug, Clone)]
pub struct UserPosts {
    post_ids: Vec<u64>,
}

impl Storable for UserPosts {
    const BOUND: ic_stable_structures::storable::Bound = Bound::Unbounded;

    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        // Handle potential deserialization failure gracefully
        Decode!(&bytes.as_ref(), Self).unwrap_or_else(|e| {
            // Log or return a default instance in case of failure
            ic_cdk::api::print(&format!("Deserialization failed: {:?}", e));
            UserPosts { post_ids: Vec::new() } // return empty vector or some default value
        })
    }
}


impl UserPosts {
    pub fn new() -> Self {
        UserPosts {
            post_ids: Vec::new(),
        }
    }

    pub fn add_post(&mut self, post_id: u64) {
        self.post_ids.push(post_id);
    }

    pub fn get_posts(&self) -> &Vec<u64> {
        &self.post_ids
    }

    pub fn remove_post(&mut self, post_id: u64) -> Result<(), String> {
        let initial_len = self.post_ids.len();
        
        self.post_ids.retain(|&x| x != post_id);
    
        if self.post_ids.len() < initial_len {
            Ok(())
        } else {
            Err("Post ID not found".to_string())
        }
    }

    pub fn remove_all(&mut self)  -> Result<(), String> {
        self.post_ids.clear(); // Removes all elements in the Vec

        if self.post_ids.len() == 0 {
            Ok(())
        }else{
            Err("Failed to delete everything".to_string())
        }
    }
}

// Thread-local storage for memory manager and StableBTreeMap
thread_local! {
    static MEMORY_MANAGER : RefCell<MemoryManager<DefaultMemoryImpl>> = RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));

    // Modify USER_POSTS_MAP to store UserPosts (a struct containing a Vec of post IDs)
    pub static USER_POSTS_MAP: RefCell<StableBTreeMap<Principal, UserPosts, Memory>> = RefCell::new(
        StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0))))
    );

    pub static POST_STORAGE_MAP: RefCell<StableBTreeMap<u64, Post, Memory>> = RefCell::new(
        StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(1))))
    );
}

#[update]
pub fn create_post(content: String) -> u64 {
    let principal = ic_cdk::api::caller();
    let post_id = ic_cdk::api::time();
    println!("{}",post_id);
    let new_post = Post { id: post_id, content };

    // Modify USER_POSTS_MAP to store UserPosts
    USER_POSTS_MAP.with(|map| {
        let mut user_posts = map.borrow_mut();
        
        // Check if the user already has posts (get the entry for the principal)
        if let Some(mut user_posts_entry) = user_posts.get(&principal).clone() {
            // If user already has posts, add the new post ID to their list
            user_posts_entry.add_post(post_id); // Add the new post ID to the user's list
            // Re-insert the modified UserPosts back into the map
            user_posts.insert(principal, user_posts_entry);
        } else {
            // If no posts exist, create a new UserPosts struct and insert it
            let mut new_user_posts = UserPosts::new();
            new_user_posts.add_post(post_id);
            user_posts.insert(principal, new_user_posts);
        }
    });

    // Store the post in POST_STORAGE_MAP
    POST_STORAGE_MAP.with(|map| {
        let mut storage = map.borrow_mut();
        storage.insert(post_id, new_post);
    });

    post_id
}



// Function to update an existing post
#[update]
pub fn update_post(post_id: u64, new_content: String) -> Result<(), String> {
    let principal = ic_cdk::api::caller();

    USER_POSTS_MAP.with(|user_map| {
        let user_map = user_map.borrow();
        
        if let Some(user_post_ids) = user_map.get(&principal) {
            if user_post_ids.get_posts().contains(&post_id) {
                return POST_STORAGE_MAP.with(|post_map| {
                    let mut storage = post_map.borrow_mut();
                    if let Some(mut post) = storage.get(&post_id).into_iter().next() {
                        post.content = new_content;
                        storage.insert(post_id, post);
                        Ok(())
                    } else {
                        Err("Post not found!".to_string())
                    }
                });
            } else {
                return Err("You do not own this post!".to_string());
            }
        }
        Err("Post not found or unauthorized!".to_string())
    })
}

// Function to delete a post
#[update]
pub fn delete_post(post_id: u64) -> Result<(), String> {
    let principal = ic_cdk::api::caller();

    USER_POSTS_MAP.with(|map| {
        let mut storage = map.borrow_mut();
        
        if let Some(mut user_post_ids) = storage.get(&principal) {
            // Check if the post_id exists in the user's post list
            if user_post_ids.get_posts().contains(&post_id) {
                
                // Attempt to remove the post ID from the user's post list
                match user_post_ids.remove_post(post_id) {
                    Ok(()) => {
                        storage.insert(principal, user_post_ids.clone());

                        // If the removal from user posts is successful, attempt to remove from post storage
                        POST_STORAGE_MAP.with(|post_map| {
                            let mut post_storage = post_map.borrow_mut();
                            
                            // If the post ID exists in the storage, remove it and return Ok
                            if post_storage.remove(&post_id).is_some() {
                                // Successfully removed the post from both maps
                                Ok(())
                            } else {
                                // Post not found in storage
                                Err("Post not found in storage!".to_string())
                            }
                        })
                    }
                    Err(err) => Err(err), // Propagate the error if unable to remove from user posts
                }
            } else {
                // The post_id was not found in the user's list of posts
                Err("Post ID not found in user's post list!".to_string())
            }
        } else {
            // No posts found for the user
            Err("User not found!".to_string())
        }
    })
}
#[update]
pub fn delete_all() -> Result<(), String> {
    let principal = ic_cdk::api::caller();

    USER_POSTS_MAP.with(|map| {
        let mut storage = map.borrow_mut();
        
        if let Some(mut user_post_ids) = storage.get(&principal) {
            // Check if the post_id exists in the user's post list
            let posts_to_delete = user_post_ids.get_posts().clone(); 
            user_post_ids.remove_all().ok(); // Ignore error since we will validate later

            storage.insert(principal, user_post_ids);
            
            POST_STORAGE_MAP.with(|post_map| {
                let mut post_storage = post_map.borrow_mut();
                for post_id in posts_to_delete {
                    post_storage.remove(&post_id);
                }
            });
            Ok(())
        } else {
            // No posts found for the user
            Err("User not found!".to_string())
        }
    })
}


// Function to retrieve all post IDs for a specific Principal
#[query]
pub fn get_paginated_posts(offset: usize, limit: usize) -> Result<Vec<(u64, String)>, String> {
    let principal = ic_cdk::api::caller();
    let posts = USER_POSTS_MAP.with(|user_map| {
        let user_map = user_map.borrow();
        user_map
            .get(&principal)
            .map(|user_posts| user_posts.get_posts().clone())
            .unwrap_or_else(Vec::new)
    });

    let total_posts = posts.len();
    if offset >= total_posts {
        return Ok(vec![]); // If offset is beyond available posts, return empty list
    }

    // Slice the vector to apply pagination
    let paginated_posts = &posts[offset..std::cmp::min(offset + limit, total_posts)];

    let mut result = Vec::new();

    ic_cdk::api::print(&format!(
        "Fetching posts for user {} with offset {} and limit {}",
        principal, offset, limit
    ));

    for &pid in paginated_posts {
        if let Some(post_data) = POST_STORAGE_MAP.with(|map| map.borrow().get(&pid)) {
            result.push((pid, post_data.content.clone()));
        }
    }

    ic_cdk::api::print(&format!("Returning paginated posts: {:?}", result));

    Ok(result)
}


#[query]
pub fn get_all_posts() -> Result<Vec<(u64,String)>, String> {
    let principal = ic_cdk::api::caller();
    let posts = USER_POSTS_MAP.with(|user_map| {
        let user_map = user_map.borrow();
        user_map
            .get(&principal)
            .map(|user_posts| user_posts.get_posts().clone())
            .unwrap_or_else(Vec::new)
    });

    // Ok(posts)
    let mut result = Vec::new();

    ic_cdk::api::print(&format!("Found post IDs for user {}: {:?}", principal, posts));

    // Iterate over the post IDs
    for pid in posts {
        if let Some(post_data) = POST_STORAGE_MAP.with(|map| map.borrow().get(&pid)) {
            ic_cdk::api::print(&format!("Found post with ID {}: {:?}", pid, post_data));
            result.push((pid, post_data.content.clone())); // Clone the data if needed
        } else {
            ic_cdk::api::print(&format!("Post with ID {} not found in storage", pid));
            
        }
    }
    // Return the collected result
    ic_cdk::api::print(&format!("Found post IDs for user  {:?}", result.clone()));

    Ok(result)
}

ic_cdk::export_candid!();
