use k8s_openapi::api::batch::v1 as batchapi;
use k8s_openapi::api::core::v1 as api;
use k8s_openapi::apimachinery::pkg::apis::meta::v1 as meta;
use kube::api::{DeleteParams, PatchParams, PostParams};
use kube::client::APIClient;
use log::info;
use std::collections::BTreeMap;

use crate::schematic::component::Component;
use crate::workload_type::{InstigatorResult, ParamMap};

/// WorkloadMetadata contains common data about a workload.
///
/// Individual workload types can embed this field.
pub struct WorkloadMetadata {
    /// Name is the name of the release
    pub name: String,
    /// Component name is the name of this particular workload component
    pub component_name: String,
    /// Instance name is the name of this component's instance (unique name)
    pub instance_name: String,
    /// Namespace is the Kubernetes namespace into which this component should
    /// be placed.
    pub namespace: String,
    /// Definition is the definition of the component.
    pub definition: Component,
    /// Client is the Kubernetes API client
    pub client: APIClient,
    /// Params contains a map of parameters that were supplied for this workload
    pub params: ParamMap,
    /// Owner Ref is the Kubernetes owner reference
    ///
    /// This tells Kubenretes what object "owns" this workload and is responsible
    /// for cleaning it up.
    pub owner_ref: Option<Vec<meta::OwnerReference>>,
    pub annotations: Option<Labels>,
}

type Labels = BTreeMap<String, String>;

/// JobBuilder builds new jobs specific to Scylla
///
/// This hides many of the details of building a Job, exposing only
/// parameters common to Scylla workload types.
pub(crate) struct JobBuilder {
    component: Component,
    labels: Labels,
    annotations: Option<Labels>,
    name: String,
    restart_policy: String,
    owner_ref: Option<Vec<meta::OwnerReference>>,
    parallelism: Option<i32>,
    param_vals: ParamMap,
}

impl JobBuilder {
    /// Create a JobBuilder
    pub fn new(instance_name: String, component: Component) -> Self {
        JobBuilder {
            component,
            name: instance_name,
            labels: Labels::new(),
            annotations: None,
            restart_policy: "Never".to_string(),
            owner_ref: None,
            parallelism: None,
            param_vals: BTreeMap::new(),
        }
    }
    /// Add labels
    pub fn labels(mut self, labels: Labels) -> Self {
        self.labels = labels;
        self
    }

    /// Add annotations.
    ///
    /// In Kubernetes, these will be added to the pod specification.
    pub fn annotations(mut self, annotations: Option<Labels>) -> Self {
        self.annotations = annotations;
        self
    }

    pub fn parameter_map(mut self, param_vals: ParamMap) -> Self {
        self.param_vals = param_vals;
        self
    }
    /// Set the restart policy
    pub fn restart_policy(mut self, policy: String) -> Self {
        self.restart_policy = policy;
        self
    }
    /// Set the owner refence for the job and the pod
    pub fn owner_ref(mut self, owner: Option<Vec<meta::OwnerReference>>) -> Self {
        self.owner_ref = owner;
        self
    }
    /// Set the parallelism
    pub fn parallelism(mut self, count: i32) -> Self {
        self.parallelism = Some(count);
        self
    }
    fn to_job(&self) -> batchapi::Job {
        batchapi::Job {
            // TODO: Could make this generic.
            metadata: Some(meta::ObjectMeta {
                name: Some(self.name.clone()),
                labels: Some(self.labels.clone()),
                owner_references: self.owner_ref.clone(),
                ..Default::default()
            }),
            spec: Some(batchapi::JobSpec {
                backoff_limit: Some(4),
                parallelism: self.parallelism,
                template: api::PodTemplateSpec {
                    metadata: Some(meta::ObjectMeta {
                        name: Some(self.name.clone()),
                        labels: Some(self.labels.clone()),
                        annotations: self.annotations.clone(),
                        owner_references: self.owner_ref.clone(),
                        ..Default::default()
                    }),
                    spec: Some(self.component.to_pod_spec_with_policy(
                        self.param_vals.clone(),
                        self.restart_policy.clone(),
                    )),
                },
                ..Default::default()
            }),
            ..Default::default()
        }
    }
    pub fn do_request(self, client: APIClient, namespace: String, phase: &str) -> InstigatorResult {
        let job = self.to_job();
        // Right now, the Batch API is not transparent through Kube.
        // Next release of Kube will fix this
        let batch = kube::api::RawApi {
            group: "batch".into(),
            resource: "jobs".into(),
            prefix: "apis".into(),
            namespace: Some(namespace),
            version: "v1".into(),
        };
        let req;
        match phase {
            "modify" => {
                let pp = kube::api::PatchParams::default();
                req = batch.patch(self.name.as_str(), &pp, serde_json::to_vec(&job)?)?;
            }
            "delete" => {
                let pp = kube::api::DeleteParams::default();
                req = batch.delete(self.name.as_str(), &pp)?;
            }
            _ => {
                let pp = kube::api::PostParams::default();
                req = batch.create(&pp, serde_json::to_vec(&job)?)?;
            }
        }
        client.request::<batchapi::Job>(req)?;
        Ok(())
    }
}

pub struct ServiceBuilder {
    component: Component,
    labels: Labels,
    selector: Labels,
    name: String,
    owner_ref: Option<Vec<meta::OwnerReference>>,
}

impl ServiceBuilder {
    pub fn new(instance_name: String, component: Component) -> Self {
        ServiceBuilder {
            component,
            name: instance_name,
            labels: Labels::new(),
            selector: Labels::new(),
            owner_ref: None,
        }
    }
    pub fn labels(mut self, labels: Labels) -> Self {
        self.labels = labels;
        self
    }
    pub fn select_labels(mut self, labels: Labels) -> Self {
        self.selector = labels;
        self
    }
    pub fn owner_reference(mut self, owner_ref: Option<Vec<meta::OwnerReference>>) -> Self {
        self.owner_ref = owner_ref;
        self
    }
    fn to_service(&self) -> Option<api::Service> {
        self.component.clone().listening_port().and_then(|port| {
            Some(api::Service {
                metadata: Some(meta::ObjectMeta {
                    name: Some(self.name.clone()),
                    labels: Some(self.labels.clone()),
                    owner_references: self.owner_ref.clone(),
                    ..Default::default()
                }),
                spec: Some(api::ServiceSpec {
                    selector: Some(self.selector.clone()),
                    ports: Some(vec![port.to_service_port()]),
                    ..Default::default()
                }),
                ..Default::default()
            })
        })
    }
    pub fn do_request(self, client: APIClient, namespace: String, phase: &str) -> InstigatorResult {
        match self.to_service() {
            Some(svc) => {
                log::debug!("Service:\n{}", serde_json::to_string_pretty(&svc).unwrap());
                match phase {
                    "modify" => {
                        let pp = PatchParams::default();
                        kube::api::Api::v1Service(client)
                            .within(namespace.as_str())
                            .patch(self.name.as_str(), &pp, serde_json::to_vec(&svc.spec)?)?;
                        Ok(())
                    }
                    "delete" => {
                        let pp = DeleteParams::default();
                        kube::api::Api::v1Service(client)
                            .within(namespace.as_str())
                            .delete(self.name.as_str(), &pp)?;
                        Ok(())
                    }
                    _ => {
                        let pp = PostParams::default();
                        kube::api::Api::v1Service(client)
                            .within(namespace.as_str())
                            .create(&pp, serde_json::to_vec(&svc)?)?;
                        Ok(())
                    }
                }
            }
            // No service to create
            None => {
                info!("Not attaching service to pod with no container ports.");
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod test {
    use crate::schematic::component::{Component, Container, Port, PortProtocol};
    use crate::workload_type::workload_builder::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;

    #[test]
    fn test_job_builder() {
        let job = JobBuilder::new("testjob".into(), skeleton_component())
            .labels(skeleton_labels())
            .restart_policy("OnError".into())
            .owner_ref(skeleton_owner_ref())
            .parallelism(2)
            .to_job();
        assert_eq!(
            job.metadata
                .clone()
                .expect("metadata")
                .labels
                .expect("labels")
                .len(),
            2
        );
        assert_eq!(
            job.metadata
                .clone()
                .expect("metadata")
                .owner_references
                .expect("owners")
                .len(),
            1
        );
        assert_eq!(job.spec.clone().expect("spec").parallelism, Some(2));
        assert_eq!(
            job.spec
                .clone()
                .unwrap()
                .template
                .spec
                .expect("spec")
                .restart_policy,
            Some("OnError".into())
        );
    }

    #[test]
    fn test_service_builder() {
        let svc = ServiceBuilder::new("test".into(), skeleton_component())
            .labels(skeleton_labels())
            .select_labels(skeleton_select_labels())
            .owner_reference(skeleton_owner_ref())
            .to_service()
            .expect("service");
        assert_eq!(
            svc.metadata
                .clone()
                .expect("metadata")
                .labels
                .expect("labels")
                .len(),
            2
        );
        assert_eq!(
            svc.spec
                .clone()
                .expect("metadata")
                .selector
                .expect("select_labels")
                .len(),
            1
        );
        assert_eq!(
            svc.metadata
                .clone()
                .expect("metadata")
                .owner_references
                .expect("owners")
                .len(),
            1
        );
    }

    #[test]
    fn test_service_builder_no_port() {
        let c = Component {
            workload_type: "worker".into(),
            os_type: "linux".into(),
            arch: "amd64".into(),
            parameters: vec![],
            containers: vec![Container {
                name: "foo".into(),
                ports: vec![], // <-- No port, no service created.
                env: vec![],
                cmd: None,
                args: None,
                image: "test/foo:latest".into(),
                image_pull_secret: None,
                liveness_probe: None,
                readiness_probe: None,
                resources: Default::default(),
            }],
            workload_settings: vec![],
        };
        assert!(ServiceBuilder::new("test".into(), c)
            .labels(skeleton_labels())
            .owner_reference(skeleton_owner_ref())
            .to_service()
            .is_none());
    }

    fn skeleton_labels() -> BTreeMap<String, String> {
        let mut labels = BTreeMap::new();
        labels.insert("first".into(), "one".into());
        labels.insert("second".into(), "two".into());
        labels
    }
    fn skeleton_select_labels() -> BTreeMap<String, String> {
        let mut labels = BTreeMap::new();
        labels.insert("first".into(), "one".into());
        labels
    }
    fn skeleton_component() -> Component {
        Component {
            workload_type: "worker".into(),
            os_type: "linux".into(),
            arch: "amd64".into(),
            parameters: vec![],
            containers: vec![Container {
                name: "foo".into(),
                ports: vec![Port {
                    container_port: 80,
                    name: "http".into(),
                    protocol: PortProtocol::TCP,
                }],
                cmd: None,
                args: None,
                env: vec![],
                image: "test/foo:latest".into(),
                image_pull_secret: None,
                liveness_probe: None,
                readiness_probe: None,
                resources: Default::default(),
            }],
            workload_settings: vec![],
        }
    }
    fn skeleton_owner_ref() -> Option<Vec<OwnerReference>> {
        Some(vec![OwnerReference {
            ..Default::default()
        }])
    }
}
