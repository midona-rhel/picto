# PBI-506: Collections as aggregate projection

## Priority
P0

## Problem
Collections still behave like a hidden special case instead of an explicit aggregate.

## Goal
Make collections a backend-owned aggregate read model.

## Implementation
1. Collection is a special media entity with membership.
2. Backend returns collection summaries as derived read models.
3. General scopes exclude grouped members.
4. Collection scope includes grouped members plus collection summary metadata.

## Acceptance Criteria
1. Collection logic is query and read-model driven, not scattered UI exceptions.
2. Collection summary is backend-owned.
