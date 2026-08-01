# API Reference

## Core

The machine itself, its per-run execution context, and the event that drives a transition.

::: statem.StateMachine

::: statem.ExecutionContext

::: statem.Signal

## Configuration

The (Pydantic-validated) shapes that make up the `config` dict passed to `StateMachine.from_dict`.

::: statem.StateConfig

::: statem.TransitionConfig

## Registries

Where named guard/action callables are registered and looked up by the engine.

::: statem.ActionRegistry

::: statem.GuardRegistry

## Errors

::: statem.GuardError

::: statem.TransitionError

## Tracing a run

::: statem.ResultEntry

::: statem.show_transitions
