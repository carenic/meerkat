//! Composition dispatcher — THE typed execution path for routed effects.
//!
//! Wave-b V2 rebuilds composition dispatch as a typed, *mandatory* runtime seam.
//! The deleted `composition_dispatch.rs` and `recompute_mob_peer_overlay*.rs`
//! (wave-a tombstones `ce2dbe35e` / `f5e366f38`) were stringly-typed helpers
//! that callers opted into. This module is their structural opposite:
//!
//! * **Typed end-to-end.** Producer identity is [`ProducerInstance`] carrying
//!   typed [`CompositionId`], [`MachineInstanceId`], [`MachineId`]. Effects
//!   travel as [`EffectPayload<E>`] where `E` is the producer composition's
//!   typed seam-effect sum (the [`ProducerEffect`] trait bound). Route
//!   resolution returns a typed [`RoutedInputDescriptor`] carrying
//!   [`MachineInstanceId`] / [`InputVariantId`] / `Vec<(FieldId, FieldId)>`.
//! * **Mandatory, not optional.** The trait has no fallback surface. Routed
//!   effects whose route is declared in the composition schema MUST resolve
//!   through the dispatcher; unresolved routes are a typed
//!   [`DispatchRefusal::UnresolvedRoute`] error, not a silent drop.
//!   Signal-kind routes live on a separate index inside [`RouteTable`] and
//!   never reach the input dispatcher — signals are the signal surface's
//!   responsibility and are flagged as a wave-b follow-up.
//! * **Compile-time presence/absence.** A `MeerkatMachine` either has a
//!   composition dispatcher attached (via the `with_composition` constructor)
//!   or it does not (the standalone / single-machine test path). The two
//!   cases are distinguished by a typed [`CompositionBinding`] discriminant,
//!   never by `Option<Arc<dyn CompositionDispatcher>>`.
//!
//! The default catalog-backed dispatcher ([`CatalogCompositionDispatcher`])
//! consumes a [`RouteTable`] built from any
//! [`meerkat_machine_schema::CompositionSchema`] and delivers each resolved
//! [`RoutedInputDescriptor`] to a per-consumer-instance [`ConsumerSurface`]
//! supplied at wire-up. The per-composition codegen module emitted by
//! `meerkat-machine-codegen` (B-4 + B-4b) plugs in as the
//! [`ProducerEffect`] implementation — `route_to_input` is equivalent to
//! consulting the [`RouteTable`] built from the same schema.

pub mod route_table;

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use meerkat_machine_schema::identity::{
    CompositionId, EffectVariantId, FieldId, InputVariantId, MachineId, MachineInstanceId, RouteId,
};
use thiserror::Error;

pub use route_table::{RouteTable, RouteTableError, RoutedInputDescriptor};

/// Typed identity of the producing machine instance inside a composition.
///
/// Unlike the deleted string-keyed helpers, every field is a typed newtype
/// so cross-instance mixups are rejected at compile time.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProducerInstance {
    /// Composition that contains the producer.
    pub composition: CompositionId,
    /// Instance id of the producer *within* `composition`.
    pub instance_id: MachineInstanceId,
    /// Underlying machine name (the schema this instance is an instance of).
    pub machine: MachineId,
}

/// Typed effect payload. Generic over the producer composition's seam-effect
/// sum (see the codegen-emitted `{Composition}Effect` enum).
///
/// The variant carries the typed [`EffectVariantId`] alongside the body so
/// the dispatcher can look up a route without pattern-matching on the
/// producer's effect enum (the [`ProducerEffect`] trait hides that under
/// [`ProducerEffect::variant_id`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectPayload<E> {
    /// Producer emitted a typed effect variant.
    Emitted {
        /// Typed variant id (matches the producer's effect enum tag).
        variant: EffectVariantId,
        /// The typed effect body.
        body: E,
    },
}

impl<E: ProducerEffect> EffectPayload<E> {
    /// Borrow the typed variant id.
    pub fn variant(&self) -> &EffectVariantId {
        match self {
            Self::Emitted { variant, .. } => variant,
        }
    }

    /// Borrow the typed body.
    pub fn body(&self) -> &E {
        match self {
            Self::Emitted { body, .. } => body,
        }
    }
}

/// Typed route key: `(composition, route)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RouteKey {
    pub composition: CompositionId,
    pub route_id: RouteId,
}

/// Marker trait for the seam-effect sum emitted by
/// `meerkat-machine-codegen::render_composition_driver`. Producer effect
/// enums implement this to expose the typed variant id alongside their
/// domain body — the dispatcher consults it without inspecting the enum.
pub trait ProducerEffect: fmt::Debug + Send + Sync + 'static {
    /// Typed variant id for this effect value.
    ///
    /// The codegen emits one arm per distinct `{producer_instance}::{variant}`
    /// pair; implementers return the matching [`EffectVariantId`]. This is
    /// the single handle the dispatcher needs to resolve the route without
    /// case-matching on the producer's concrete enum.
    fn variant_id(&self) -> EffectVariantId;

    /// Borrow a field value by [`FieldId`].
    ///
    /// Used by the dispatcher to project producer fields into the typed
    /// consumer input as declared by the composition's route bindings.
    /// Returns `None` if the requested field is not present on this
    /// variant. The dispatcher treats that as
    /// [`DispatchRefusal::MissingProducerField`].
    fn field(&self, id: &FieldId) -> Option<FieldValue<'_>>;
}

/// Typed view over a producer-field value projected through a route binding.
///
/// The `ProducerEffect::field` implementation returns one of these so the
/// dispatcher can move the value into the consumer input without a
/// `serde_json::Value` round-trip. The variant set is intentionally small;
/// richer shapes are expressed by the producer keeping the typed value
/// inside its effect body and the consumer accepting it via the same
/// typed enum (the codegen emits the matching types on both sides).
#[derive(Debug, Clone)]
pub enum FieldValue<'a> {
    /// Borrowed string slice (owning producer retains the backing `String`).
    Str(&'a str),
    /// Unsigned 64-bit integer.
    U64(u64),
    /// Signed 64-bit integer.
    I64(i64),
    /// Boolean flag.
    Bool(bool),
    /// Opaque typed handle — producer and consumer agree on the Rust type.
    /// The dispatcher moves the `Arc<dyn Any>` across without inspecting it.
    /// This is *not* a `serde_json::Value` escape hatch: the contained Rust
    /// type is determined by the typed route binding, not by ad-hoc JSON.
    Opaque(Arc<dyn std::any::Any + Send + Sync>),
}

/// Outcome when a routed effect is successfully dispatched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchOutcome {
    /// Route that was resolved for this effect.
    pub route: RouteKey,
    /// Target consumer instance the typed input was delivered to.
    pub consumer: MachineInstanceId,
    /// Typed input variant applied on the consumer.
    pub applied_input: InputVariantId,
}

/// Reasons the dispatcher refuses a routed effect.
///
/// Unlike the deleted helper path, there is no "silently drop unknown
/// effects" arm. Every failure is a typed variant so callers and RMAT
/// audits can enumerate them without parsing error strings.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DispatchRefusal {
    /// The producer is not registered for this dispatcher's composition.
    #[error(
        "dispatcher composition {expected} does not match producer composition {actual}"
    )]
    CompositionMismatch {
        expected: CompositionId,
        actual: CompositionId,
    },
    /// No input-kind route is declared for `(producer.instance_id, variant)`.
    #[error(
        "no input route declared for producer {instance} effect variant {variant} in composition {composition}"
    )]
    UnresolvedRoute {
        composition: CompositionId,
        instance: MachineInstanceId,
        variant: EffectVariantId,
    },
    /// A route-binding references a producer field that the effect body did
    /// not supply (via [`ProducerEffect::field`]).
    #[error(
        "route {route} requires producer field {field} on variant {variant}, not provided"
    )]
    MissingProducerField {
        route: RouteId,
        variant: EffectVariantId,
        field: FieldId,
    },
    /// No [`ConsumerSurface`] is registered for the resolved target
    /// instance. This is a wiring bug at construction time, not a runtime
    /// signal — the dispatcher refuses rather than queueing forever.
    #[error(
        "no consumer surface registered for target instance {instance} in composition {composition}"
    )]
    UnwiredConsumer {
        composition: CompositionId,
        instance: MachineInstanceId,
    },
    /// The consumer surface rejected the typed input (e.g. because the
    /// consumer machine is no longer accepting inputs). The inner message
    /// is the consumer-side rejection reason and is opaque to the
    /// dispatcher — typed by the consumer's own error.
    #[error("consumer {instance} refused input {variant}: {reason}")]
    ConsumerRefused {
        instance: MachineInstanceId,
        variant: InputVariantId,
        reason: String,
    },
}

/// Delivery surface for one consumer instance inside a composition.
///
/// A consumer (e.g. the `meerkat` machine instance when mob routes
/// `RequestRuntimeBinding` at it) implements this trait and registers an
/// instance at composition wire-up. The dispatcher invokes it exactly once
/// per resolved [`RoutedInput`]. The implementation is responsible for
/// materializing the consumer-side typed input — the dispatcher only moves
/// typed data across the seam.
#[async_trait]
pub trait ConsumerSurface: Send + Sync {
    /// Instance id this surface serves. The dispatcher matches against
    /// [`RoutedInput::instance_id`] to pick the right surface.
    fn instance_id(&self) -> &MachineInstanceId;

    /// Apply a typed routed input. `projected_fields` carries the per-
    /// consumer-field values resolved from the producer via the route's
    /// field-bindings, owned so the surface can move them into its typed
    /// input constructor.
    async fn apply_routed_input(
        &self,
        variant: InputVariantId,
        projected_fields: Vec<(FieldId, OwnedFieldValue)>,
    ) -> Result<(), String>;
}

/// Owned counterpart of [`FieldValue`] used when delivering a routed input
/// across the consumer-surface boundary. Moving owned values means the
/// consumer can construct its typed input without re-borrowing the
/// producer.
#[derive(Debug, Clone)]
pub enum OwnedFieldValue {
    Str(String),
    U64(u64),
    I64(i64),
    Bool(bool),
    Opaque(Arc<dyn std::any::Any + Send + Sync>),
}

impl<'a> FieldValue<'a> {
    /// Lift a borrowed field value into its owned counterpart, cloning the
    /// backing `&str` when required. The [`Arc<dyn Any>`] path is shared,
    /// not cloned.
    pub fn to_owned_value(&self) -> OwnedFieldValue {
        match self {
            FieldValue::Str(s) => OwnedFieldValue::Str((*s).to_owned()),
            FieldValue::U64(v) => OwnedFieldValue::U64(*v),
            FieldValue::I64(v) => OwnedFieldValue::I64(*v),
            FieldValue::Bool(v) => OwnedFieldValue::Bool(*v),
            FieldValue::Opaque(handle) => OwnedFieldValue::Opaque(Arc::clone(handle)),
        }
    }
}

/// Composition dispatcher trait.
///
/// Monomorphized over the producer composition's seam-effect sum
/// ([`CompositionDispatcher::Effect`]). Making the effect an associated type
/// (rather than a generic on the method) keeps the trait dyn-safe — a
/// `MeerkatMachine` can hold `Arc<dyn CompositionDispatcher<Effect = ...>>`
/// without leaking the machine kernel's monomorphization concerns.
#[async_trait]
pub trait CompositionDispatcher: Send + Sync {
    /// Seam-effect sum this dispatcher handles. Matches the codegen-emitted
    /// `{Composition}Effect` enum.
    type Effect: ProducerEffect;

    /// Composition id this dispatcher owns. Every [`ProducerInstance`]
    /// passed to [`CompositionDispatcher::dispatch`] must match.
    fn composition(&self) -> &CompositionId;

    /// Dispatch a routed effect. Returns [`DispatchOutcome`] on success or
    /// a typed [`DispatchRefusal`]. There is no silent-drop arm.
    async fn dispatch(
        &self,
        producer: ProducerInstance,
        effect: EffectPayload<Self::Effect>,
    ) -> Result<DispatchOutcome, DispatchRefusal>;
}

/// Typed binding attached to a runtime that holds a dispatcher.
///
/// Discriminates the "machine participates in a composition" case from the
/// "machine is standalone" case *at the type level*: no
/// `Option<Arc<dyn CompositionDispatcher>>`. Callers obtain the concrete
/// dispatcher via [`CompositionBinding::wired`] and honor
/// [`CompositionBinding::is_standalone`] to tell the two apart. The two
/// constructor halves on `MeerkatMachine` (`with_composition(...)` vs
/// `standalone(...)` / `ephemeral()` / `persistent()`) are the public
/// face of this distinction.
pub enum CompositionBinding<E: ProducerEffect> {
    /// Machine is not part of a composition. Routed-effect dispatch is not
    /// available.
    Standalone,
    /// Machine participates in a composition and owns a typed dispatcher.
    Wired(Arc<dyn CompositionDispatcher<Effect = E>>),
}

impl<E: ProducerEffect> fmt::Debug for CompositionBinding<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Standalone => f.debug_struct("CompositionBinding::Standalone").finish(),
            Self::Wired(_) => f
                .debug_struct("CompositionBinding::Wired")
                .field("dispatcher", &"<dyn CompositionDispatcher>")
                .finish(),
        }
    }
}

impl<E: ProducerEffect> CompositionBinding<E> {
    /// Report whether this machine is standalone (no composition attached).
    pub fn is_standalone(&self) -> bool {
        matches!(self, Self::Standalone)
    }

    /// Borrow the wired dispatcher, if any.
    ///
    /// Returns `None` only for [`CompositionBinding::Standalone`]. Callers
    /// that reach a routed-effect site MUST have a `Wired` binding — the
    /// type split exists so this is enforced at the construction boundary,
    /// not re-checked at every call site.
    pub fn wired(&self) -> Option<&Arc<dyn CompositionDispatcher<Effect = E>>> {
        match self {
            Self::Standalone => None,
            Self::Wired(d) => Some(d),
        }
    }
}

/// Default catalog-backed dispatcher.
///
/// Consumes a [`RouteTable`] (built from a
/// [`meerkat_machine_schema::CompositionSchema`]) plus a map of consumer
/// surfaces keyed by [`MachineInstanceId`]. Every routed effect goes
/// through the same three steps:
///
/// 1. Look up the input-kind route for `(producer.instance_id, effect.variant)`.
/// 2. Project the producer's field values into the consumer-field bindings.
/// 3. Deliver via the consumer surface registered for the target instance.
///
/// No step has a silent-drop fallback. Unresolved routes, signal-kind
/// targets, missing producer fields, and unwired consumers are all typed
/// [`DispatchRefusal`] errors.
pub struct CatalogCompositionDispatcher<E: ProducerEffect> {
    composition: CompositionId,
    table: RouteTable,
    consumers: HashMap<MachineInstanceId, Arc<dyn ConsumerSurface>>,
    _effect: std::marker::PhantomData<fn(E)>,
}

impl<E: ProducerEffect> fmt::Debug for CatalogCompositionDispatcher<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CatalogCompositionDispatcher")
            .field("composition", &self.composition)
            .field("routes", &self.table.len())
            .field("consumers", &self.consumers.len())
            .finish()
    }
}

impl<E: ProducerEffect> CatalogCompositionDispatcher<E> {
    /// Build a new dispatcher for `composition`, using `table` as the typed
    /// route index.
    pub fn new(composition: CompositionId, table: RouteTable) -> Self {
        Self {
            composition,
            table,
            consumers: HashMap::new(),
            _effect: std::marker::PhantomData,
        }
    }

    /// Register a consumer surface for a target instance.
    ///
    /// Panics are impossible — duplicate registrations replace the prior
    /// entry. (Duplicate wiring is a construction bug; the callers in
    /// wave-b prove registration happens exactly once per instance in the
    /// composition schema.)
    pub fn with_consumer(mut self, surface: Arc<dyn ConsumerSurface>) -> Self {
        self.consumers.insert(surface.instance_id().clone(), surface);
        self
    }
}

#[async_trait]
impl<E: ProducerEffect> CompositionDispatcher for CatalogCompositionDispatcher<E> {
    type Effect = E;

    fn composition(&self) -> &CompositionId {
        &self.composition
    }

    async fn dispatch(
        &self,
        producer: ProducerInstance,
        effect: EffectPayload<Self::Effect>,
    ) -> Result<DispatchOutcome, DispatchRefusal> {
        if producer.composition != self.composition {
            return Err(DispatchRefusal::CompositionMismatch {
                expected: self.composition.clone(),
                actual: producer.composition,
            });
        }

        let variant = effect.variant().clone();
        let body = effect.body();

        let descriptor = self
            .table
            .resolve(&producer.instance_id, &variant)
            .ok_or_else(|| DispatchRefusal::UnresolvedRoute {
                composition: self.composition.clone(),
                instance: producer.instance_id.clone(),
                variant: variant.clone(),
            })?;

        let mut projected: Vec<(FieldId, OwnedFieldValue)> =
            Vec::with_capacity(descriptor.bindings.len());
        for (from_field, to_field) in &descriptor.bindings {
            let value = body.field(from_field).ok_or_else(|| {
                DispatchRefusal::MissingProducerField {
                    route: descriptor.route_id.clone(),
                    variant: variant.clone(),
                    field: from_field.clone(),
                }
            })?;
            projected.push((to_field.clone(), value.to_owned_value()));
        }

        let consumer = self.consumers.get(&descriptor.instance_id).ok_or_else(|| {
            DispatchRefusal::UnwiredConsumer {
                composition: self.composition.clone(),
                instance: descriptor.instance_id.clone(),
            }
        })?;

        consumer
            .apply_routed_input(descriptor.input_variant.clone(), projected)
            .await
            .map_err(|reason| DispatchRefusal::ConsumerRefused {
                instance: descriptor.instance_id.clone(),
                variant: descriptor.input_variant.clone(),
                reason,
            })?;

        Ok(DispatchOutcome {
            route: RouteKey {
                composition: self.composition.clone(),
                route_id: descriptor.route_id.clone(),
            },
            consumer: descriptor.instance_id.clone(),
            applied_input: descriptor.input_variant.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meerkat_machine_schema::catalog::meerkat_mob_seam_composition;

    /// Hand-written stand-in for the codegen-emitted `MeerkatMobSeamEffect`
    /// sum. Matches the shape the B-4b tests pin for the live catalog:
    /// one variant per producer instance, each wrapping a typed effect
    /// body (we cover the `RequestRuntimeBinding` arm for the dispatcher
    /// path).
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum SeamEffect {
        Mob(MobEffect),
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum MobEffect {
        RequestRuntimeBinding {
            agent_runtime_id: String,
            fence_token: u64,
            generation: u64,
        },
    }

    impl ProducerEffect for SeamEffect {
        fn variant_id(&self) -> EffectVariantId {
            match self {
                Self::Mob(MobEffect::RequestRuntimeBinding { .. }) => {
                    EffectVariantId::parse("RequestRuntimeBinding").expect("slug")
                }
            }
        }

        fn field(&self, id: &FieldId) -> Option<FieldValue<'_>> {
            match self {
                Self::Mob(MobEffect::RequestRuntimeBinding {
                    agent_runtime_id,
                    fence_token,
                    generation,
                }) => match id.as_str() {
                    "agent_runtime_id" => Some(FieldValue::Str(agent_runtime_id)),
                    "fence_token" => Some(FieldValue::U64(*fence_token)),
                    "generation" => Some(FieldValue::U64(*generation)),
                    _ => None,
                },
            }
        }
    }

    #[derive(Default)]
    struct RecordingMeerkatSurface {
        log: tokio::sync::Mutex<Vec<(InputVariantId, Vec<(FieldId, OwnedFieldValue)>)>>,
    }

    #[async_trait]
    impl ConsumerSurface for RecordingMeerkatSurface {
        fn instance_id(&self) -> &MachineInstanceId {
            // leak is fine in tests: we want a 'static reference; the
            // instance_id is stable for the lifetime of the test binary.
            static ID: std::sync::OnceLock<MachineInstanceId> = std::sync::OnceLock::new();
            ID.get_or_init(|| MachineInstanceId::parse("meerkat").unwrap())
        }

        async fn apply_routed_input(
            &self,
            variant: InputVariantId,
            projected_fields: Vec<(FieldId, OwnedFieldValue)>,
        ) -> Result<(), String> {
            self.log.lock().await.push((variant, projected_fields));
            Ok(())
        }
    }

    fn mob_producer() -> ProducerInstance {
        ProducerInstance {
            composition: CompositionId::parse("meerkat_mob_seam").unwrap(),
            instance_id: MachineInstanceId::parse("mob").unwrap(),
            machine: MachineId::parse("MobMachine").unwrap(),
        }
    }

    fn sample_effect() -> EffectPayload<SeamEffect> {
        EffectPayload::Emitted {
            variant: EffectVariantId::parse("RequestRuntimeBinding").unwrap(),
            body: SeamEffect::Mob(MobEffect::RequestRuntimeBinding {
                agent_runtime_id: "rt-1".into(),
                fence_token: 7,
                generation: 3,
            }),
        }
    }

    fn build_dispatcher(
        consumer: Arc<RecordingMeerkatSurface>,
    ) -> CatalogCompositionDispatcher<SeamEffect> {
        let schema = meerkat_mob_seam_composition();
        let table = RouteTable::from_schema(&schema).expect("seam schema routes are well-formed");
        CatalogCompositionDispatcher::new(schema.name.clone(), table).with_consumer(consumer)
    }

    #[tokio::test]
    async fn dispatches_mob_routed_effect_to_meerkat_consumer() {
        let consumer = Arc::new(RecordingMeerkatSurface::default());
        let dispatcher = build_dispatcher(Arc::clone(&consumer));

        let outcome = dispatcher
            .dispatch(mob_producer(), sample_effect())
            .await
            .expect("well-formed routed effect");

        assert_eq!(outcome.consumer.as_str(), "meerkat");
        assert_eq!(outcome.applied_input.as_str(), "PrepareBindings");
        assert_eq!(outcome.route.route_id.as_str(), "binding_request_reaches_meerkat");

        let log = consumer.log.lock().await;
        assert_eq!(log.len(), 1, "dispatcher must call the consumer exactly once");
        let (variant, fields) = &log[0];
        assert_eq!(variant.as_str(), "PrepareBindings");
        let field_names: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(field_names, vec!["agent_runtime_id", "fence_token", "generation"]);
        match &fields[0].1 {
            OwnedFieldValue::Str(s) => assert_eq!(s, "rt-1"),
            other => panic!("expected Str, got {other:?}"),
        }
        match &fields[1].1 {
            OwnedFieldValue::U64(v) => assert_eq!(*v, 7),
            other => panic!("expected U64, got {other:?}"),
        }
        match &fields[2].1 {
            OwnedFieldValue::U64(v) => assert_eq!(*v, 3),
            other => panic!("expected U64, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn refuses_mismatched_composition() {
        let consumer = Arc::new(RecordingMeerkatSurface::default());
        let dispatcher = build_dispatcher(consumer);

        let mut wrong = mob_producer();
        wrong.composition = CompositionId::parse("some_other_composition").unwrap();

        let err = dispatcher
            .dispatch(wrong, sample_effect())
            .await
            .expect_err("composition mismatch");

        assert!(matches!(err, DispatchRefusal::CompositionMismatch { .. }));
    }

    #[tokio::test]
    async fn refuses_unrouted_effect_typed() {
        let consumer = Arc::new(RecordingMeerkatSurface::default());
        let dispatcher = build_dispatcher(consumer);

        // The schema has no route for `Mob::UnknownEffect`; use the well-
        // formed producer but label the variant with an id that has no
        // declared route.
        let payload = EffectPayload::Emitted {
            variant: EffectVariantId::parse("UnknownEffect").unwrap(),
            body: SeamEffect::Mob(MobEffect::RequestRuntimeBinding {
                agent_runtime_id: "rt".into(),
                fence_token: 0,
                generation: 0,
            }),
        };

        let err = dispatcher
            .dispatch(mob_producer(), payload)
            .await
            .expect_err("unresolved route");

        assert!(matches!(err, DispatchRefusal::UnresolvedRoute { .. }));
    }

    #[tokio::test]
    async fn refuses_unwired_consumer_typed() {
        // Build a dispatcher with NO consumer surface registered. The route
        // resolves but the delivery step must return UnwiredConsumer, not
        // silently succeed.
        let schema = meerkat_mob_seam_composition();
        let table = RouteTable::from_schema(&schema).unwrap();
        let dispatcher: CatalogCompositionDispatcher<SeamEffect> =
            CatalogCompositionDispatcher::new(schema.name.clone(), table);

        let err = dispatcher
            .dispatch(mob_producer(), sample_effect())
            .await
            .expect_err("unwired consumer");

        assert!(matches!(err, DispatchRefusal::UnwiredConsumer { .. }));
    }

    #[tokio::test]
    async fn standalone_binding_has_no_dispatcher() {
        let binding: CompositionBinding<SeamEffect> = CompositionBinding::Standalone;
        assert!(binding.is_standalone());
        assert!(binding.wired().is_none());
    }

    #[tokio::test]
    async fn wired_binding_exposes_dispatcher() {
        let consumer = Arc::new(RecordingMeerkatSurface::default());
        let dispatcher = Arc::new(build_dispatcher(consumer));
        let binding: CompositionBinding<SeamEffect> = CompositionBinding::Wired(dispatcher);
        assert!(!binding.is_standalone());
        assert!(binding.wired().is_some());
    }
}
