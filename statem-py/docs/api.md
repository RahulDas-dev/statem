# API Reference

## Core

The machine itself, its per-run execution context, and the event that drives a transition.

```{eval-rst}
.. autoclass:: statem.StateMachine
   :members:
   :undoc-members:

.. autoclass:: statem.Context
   :members:
   :undoc-members:

.. autoclass:: statem.Signal
   :members:
   :undoc-members:
```

## Configuration

The (Pydantic-validated) shapes that make up the `config` dict passed to `StateMachine.from_dict`.

```{eval-rst}
.. autoclass:: statem.StateConfig
   :members:
   :undoc-members:

.. autoclass:: statem.TransitionConfig
   :members:
   :undoc-members:
```

## Registries

Where named guard/action callables are registered and looked up by the engine.

```{eval-rst}
.. autoclass:: statem.ActionRegistry
   :members:
   :undoc-members:

.. autoclass:: statem.GuardRegistry
   :members:
   :undoc-members:
```

## Errors

```{eval-rst}
.. autoclass:: statem.GuardError

.. autoclass:: statem.TransitionError
```

## Tracing a run

```{eval-rst}
.. autoclass:: statem.ResultEntry
   :members:
   :undoc-members:

.. autofunction:: statem.show_transitions
```

## Diagrams

```{eval-rst}
.. autofunction:: statem.to_mermaid
```
