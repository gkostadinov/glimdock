"""Small, explicit adapter helpers; no model calls, pricing inference or content."""
import math

def a2a_state(value):
    # Accept current protobuf-style names and earlier JSON task-state spellings.
    name=str(value or '').lower().removeprefix('task_state_').replace('-','_')
    return {'submitted':'queued','working':'running','input_required':'waiting','auth_required':'waiting','completed':'done','canceled':'canceled','cancelled':'canceled','rejected':'failed','failed':'failed'}.get(name,'unknown')

def otel_usage(attributes):
    def tokens(key):
        value=attributes.get(key)
        return value if isinstance(value,int) and not isinstance(value,bool) and value>=0 else None
    return {'input_tokens':tokens('gen_ai.usage.input_tokens'),'output_tokens':tokens('gen_ai.usage.output_tokens'),'cost':None,'currency':None,'window':None,'provenance':'OpenTelemetry GenAI token attributes'}
