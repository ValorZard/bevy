use super::{BindGroup, BindGroupId, BufferId, RenderResourceId, SamplerId, TextureId};
use crate::pipeline::{BindGroupDescriptor, BindGroupDescriptorId};
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    ops::Range,
};
use uuid::Uuid;

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum RenderResourceBinding {
    Buffer {
        buffer: BufferId,
        range: Range<u64>,
        dynamic_index: Option<u32>,
    },
    Texture(TextureId),
    Sampler(SamplerId),
}

impl RenderResourceBinding {
    pub fn get_texture(&self) -> Option<TextureId> {
        if let RenderResourceBinding::Texture(texture) = self {
            Some(*texture)
        } else {
            None
        }
    }

    pub fn get_buffer(&self) -> Option<BufferId> {
        if let RenderResourceBinding::Buffer { buffer, .. } = self {
            Some(*buffer)
        } else {
            None
        }
    }

    pub fn get_sampler(&self) -> Option<SamplerId> {
        if let RenderResourceBinding::Sampler(sampler) = self {
            Some(*sampler)
        } else {
            None
        }
    }
}

impl Hash for RenderResourceBinding {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            RenderResourceBinding::Buffer {
                buffer,
                range,
                dynamic_index: _, // dynamic_index is not a part of the binding
            } => {
                RenderResourceId::from(*buffer).hash(state);
                range.hash(state);
            }
            RenderResourceBinding::Texture(texture) => {
                RenderResourceId::from(*texture).hash(state);
            }
            RenderResourceBinding::Sampler(sampler) => {
                RenderResourceId::from(*sampler).hash(state);
            }
        }
    }
}

#[derive(Eq, PartialEq, Debug)]
pub enum BindGroupStatus {
    Changed(BindGroupId),
    Unchanged(BindGroupId),
    NoMatch,
}

// PERF: if the bindings are scoped to a specific pipeline layout, then names could be replaced with indices here for a perf boost
#[derive(Eq, PartialEq, Debug, Default, Clone)]
pub struct RenderResourceBindings {
    // TODO: remove this. it shouldn't be needed anymore
    pub id: RenderResourceBindingsId,
    bindings: HashMap<String, RenderResourceBinding>,
    // TODO: remove this
    vertex_buffers: HashMap<String, (BufferId, Option<BufferId>)>,
    bind_groups: HashMap<BindGroupId, BindGroup>,
    bind_group_descriptors: HashMap<BindGroupDescriptorId, BindGroupId>,
    dirty_bind_groups: HashSet<BindGroupId>,
    // TODO: remove this
    // pub pipeline_specialization: PipelineSpecialization,
}

impl RenderResourceBindings {
    pub fn get(&self, name: &str) -> Option<&RenderResourceBinding> {
        self.bindings.get(name)
    }

    pub fn set(&mut self, name: &str, binding: RenderResourceBinding) {
        self.try_set_dirty(name, &binding);
        self.bindings.insert(name.to_string(), binding);
    }

    fn try_set_dirty(&mut self, name: &str, binding: &RenderResourceBinding) {
        if let Some(current_binding) = self.bindings.get(name) {
            if current_binding != binding {
                // TODO: this is crude. we shouldn't need to invalidate all bind groups
                for id in self.bind_groups.keys() {
                    self.dirty_bind_groups.insert(*id);
                }
            }
        }
    }

    pub fn extend(&mut self, render_resource_bindings: &RenderResourceBindings) {
        for (name, binding) in render_resource_bindings.bindings.iter() {
            self.set(name, binding.clone());
        }

        for (name, (vertex_buffer, index_buffer)) in render_resource_bindings.vertex_buffers.iter()
        {
            self.set_vertex_buffer(name, *vertex_buffer, index_buffer.clone());
        }
    }

    pub fn get_vertex_buffer(&self, name: &str) -> Option<(BufferId, Option<BufferId>)> {
        self.vertex_buffers.get(name).cloned()
    }

    pub fn set_vertex_buffer(
        &mut self,
        name: &str,
        vertex_buffer: BufferId,
        index_buffer: Option<BufferId>,
    ) {
        self.vertex_buffers
            .insert(name.to_string(), (vertex_buffer, index_buffer));
    }

    fn create_bind_group(&mut self, descriptor: &BindGroupDescriptor) -> BindGroupStatus {
        let bind_group = self.build_bind_group(descriptor);
        if let Some(bind_group) = bind_group {
            let id = bind_group.id;
            self.bind_groups.insert(id, bind_group);
            self.bind_group_descriptors.insert(descriptor.id, id);
            BindGroupStatus::Changed(id)
        } else {
            BindGroupStatus::NoMatch
        }
    }

    pub fn update_bind_group(
        &mut self,
        bind_group_descriptor: &BindGroupDescriptor,
    ) -> BindGroupStatus {
        if let Some(id) = self.bind_group_descriptors.get(&bind_group_descriptor.id) {
            if self.dirty_bind_groups.contains(id) {
                self.dirty_bind_groups.remove(id);
                self.create_bind_group(bind_group_descriptor)
            } else {
                BindGroupStatus::Unchanged(*id)
            }
        } else {
            self.create_bind_group(bind_group_descriptor)
        }
    }

    pub fn get_bind_group(&self, id: BindGroupId) -> Option<&BindGroup> {
        self.bind_groups.get(&id)
    }

    pub fn get_descriptor_bind_group(&self, id: BindGroupDescriptorId) -> Option<&BindGroup> {
        self.bind_group_descriptors
            .get(&id)
            .and_then(|bind_group_id| self.get_bind_group(*bind_group_id))
    }

    fn build_bind_group(&self, bind_group_descriptor: &BindGroupDescriptor) -> Option<BindGroup> {
        let mut bind_group_builder = BindGroup::build();
        for binding_descriptor in bind_group_descriptor.bindings.iter() {
            if let Some(binding) = self.get(&binding_descriptor.name) {
                bind_group_builder =
                    bind_group_builder.add_binding(binding_descriptor.index, binding.clone());
            } else {
                return None;
            }
        }

        Some(bind_group_builder.finish())
    }
}

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone)]
pub struct RenderResourceBindingsId(Uuid);

impl Default for RenderResourceBindingsId {
    fn default() -> Self {
        RenderResourceBindingsId(Uuid::new_v4())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{BindType, BindingDescriptor, UniformProperty, UniformPropertyType};

    #[test]
    fn test_bind_groups() {
        let bind_group_descriptor = BindGroupDescriptor::new(
            0,
            vec![
                BindingDescriptor {
                    index: 0,
                    name: "a".to_string(),
                    bind_type: BindType::Uniform {
                        dynamic: false,
                        properties: vec![UniformProperty {
                            name: "A".to_string(),
                            property_type: UniformPropertyType::Struct(vec![UniformProperty {
                                name: "".to_string(),
                                property_type: UniformPropertyType::Mat4,
                            }]),
                        }],
                    },
                },
                BindingDescriptor {
                    index: 1,
                    name: "b".to_string(),
                    bind_type: BindType::Uniform {
                        dynamic: false,
                        properties: vec![UniformProperty {
                            name: "B".to_string(),
                            property_type: UniformPropertyType::Float,
                        }],
                    },
                },
            ],
        );

        let resource1 = RenderResourceBinding::Texture(TextureId::new());
        let resource2 = RenderResourceBinding::Texture(TextureId::new());
        let resource3 = RenderResourceBinding::Texture(TextureId::new());
        let resource4 = RenderResourceBinding::Texture(TextureId::new());

        let mut bindings = RenderResourceBindings::default();
        bindings.set("a", resource1.clone());
        bindings.set("b", resource2.clone());

        let mut different_bindings = RenderResourceBindings::default();
        different_bindings.set("a", resource3.clone());
        different_bindings.set("b", resource4.clone());

        let mut equal_bindings = RenderResourceBindings::default();
        equal_bindings.set("a", resource1.clone());
        equal_bindings.set("b", resource2.clone());

        let status = bindings.update_bind_group(&bind_group_descriptor);
        let id = if let BindGroupStatus::Changed(id) = status {
            id
        } else {
            panic!("expected a changed bind group");
        };

        let different_bind_group_status =
            different_bindings.update_bind_group(&bind_group_descriptor);
        if let BindGroupStatus::Changed(different_bind_group_id) = different_bind_group_status {
            assert_ne!(
                id, different_bind_group_id,
                "different bind group shouldn't have the same id"
            );
            different_bind_group_id
        } else {
            panic!("expected a changed bind group");
        };

        let equal_bind_group_status = equal_bindings.update_bind_group(&bind_group_descriptor);
        if let BindGroupStatus::Changed(equal_bind_group_id) = equal_bind_group_status {
            assert_eq!(
                id, equal_bind_group_id,
                "equal bind group should have the same id"
            );
        } else {
            panic!("expected a changed bind group");
        };

        let mut unmatched_bindings = RenderResourceBindings::default();
        unmatched_bindings.set("a", resource1.clone());
        let unmatched_bind_group_status =
            unmatched_bindings.update_bind_group(&bind_group_descriptor);
        assert_eq!(unmatched_bind_group_status, BindGroupStatus::NoMatch);
    }
}