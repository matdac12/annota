# Triage labels

The skills speak in five canonical triage roles. They map one-to-one to Traccia's global labels:

| Role in mattpocock/skills | Label in Traccia | Meaning |
| --- | --- | --- |
| `needs-triage` | `needs-triage` | Maintainer needs to evaluate this issue |
| `needs-info` | `needs-info` | Waiting on the reporter for more information |
| `ready-for-agent` | `ready-for-agent` | Fully specified, ready for an AFK agent |
| `ready-for-human` | `ready-for-human` | Needs Mattia (hand checks, timed tests, his machine) |
| `wontfix` | `wontfix` | Will not be actioned |

If one is missing, create it with `save_issue_label` on first use.
