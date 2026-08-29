//! Audit queries - Cypher queries for detecting issues.

/// Claims without verification (Gate A audit).
/// Returns claims that have no verification proving them.
pub const ORPHAN_CLAIMS: &str = r#"
    MATCH (c:Claim)
    WHERE c.has_evidence = false
      OR NOT EXISTS { (v:Verification)-[:PROVES]->(c) }
    RETURN c.id as claim_id, 
           c.content as content, 
           c.ts as created_at
    ORDER BY c.ts DESC
"#;

/// Files edited without reading first (Gate B audit).
/// Ghost edits - patches that modify files not read before.
pub const GHOST_EDITS: &str = r#"
    MATCH (p:Patch)-[:MODIFIES]->(f:File)
    WHERE NOT EXISTS {
        MATCH (e:Event)-[:READS]->(f)
        WHERE e.ts < p.applied_at
    }
    RETURN p.id as patch_id, 
           f.path as file_path, 
           p.applied_at as applied_at
    ORDER BY p.applied_at DESC
"#;

/// Patches that modify files outside declared scope (Gate C audit).
pub const SCOPE_VIOLATIONS: &str = r#"
    MATCH (p:Patch)-[:MODIFIES]->(touched:File)
    MATCH (scope_event:Event)-[:DEFINES]->(s:Scope)
    WHERE scope_event.ts < p.applied_at
    WITH p, touched, s, scope_event
    ORDER BY scope_event.ts DESC
    LIMIT 1
    WHERE NOT EXISTS { (s)-[:ALLOWS]->(touched) }
    RETURN p.id as patch_id, 
           touched.path as violated_path, 
           s.id as scope_id
"#;

/// Patches without verification within time window (Gate D audit).
pub const UNVERIFIED_PATCHES: &str = r#"
    MATCH (p:Patch {status: 'applied'})
    WHERE NOT EXISTS {
        MATCH (v:Verification)-[:VERIFIES]->(p)
    }
    RETURN p.id as patch_id, 
           p.applied_at as applied_at
    ORDER BY p.applied_at DESC
"#;

/// Patches with failed verification not reverted (Gate E audit).
pub const FAILED_NOT_REVERTED: &str = r#"
    MATCH (v:Verification {passed: false})-[:VERIFIES]->(p:Patch)
    WHERE p.status = 'applied'
      AND NOT EXISTS { (:Patch)-[:REVERTS]->(p) }
    RETURN p.id as patch_id, 
           v.id as verification_id, 
           p.applied_at as applied_at,
           v.ts as failed_at
"#;

/// Blast radius - what files depend on a given file.
/// TODO: Requires :IMPORTS relationship which is not yet projected.
/// Enable after implementing symbol/import analysis in projector.
pub const BLAST_RADIUS: &str = r#"
    MATCH path = (f:File {path: $path})<-[:IMPORTS*1..5]-(dependent:File)
    RETURN DISTINCT dependent.path as dependent_path, 
           length(path) as depth
    ORDER BY depth
"#;

/// Timeline of changes by a specific agent.
pub const AGENT_TIMELINE: &str = r#"
    MATCH (e:Event {agent_id: $agent_id})-[r]->(target)
    RETURN e.ts as timestamp, 
           e.kind as event_kind, 
           type(r) as relation, 
           labels(target)[0] as target_type,
           e.description as description
    ORDER BY e.ts DESC
    LIMIT $limit
"#;

/// Files modified without tests in the same session.
pub const FILES_WITHOUT_TESTS: &str = r#"
    MATCH (p:Patch)-[:MODIFIES]->(f:File)
    WHERE f.path ENDS WITH '.rs'
      AND NOT EXISTS {
        MATCH (v:Verification {passed: true})
        WHERE v.ts > p.applied_at 
          AND v.ts < p.applied_at + duration('PT30M')
      }
    RETURN f.path as file_path, 
           count(p) as patch_count,
           max(p.applied_at) as last_modified
    ORDER BY patch_count DESC
"#;

/// Dashboard summary counts.
pub const DASHBOARD_COUNTS: &str = r#"
    MATCH (c:Claim)
    OPTIONAL MATCH (orphan:Claim) WHERE orphan.has_evidence = false
    OPTIONAL MATCH (p:Patch {status: 'applied'})
    OPTIONAL MATCH (v:Verification)
    RETURN count(DISTINCT c) as total_claims,
           count(DISTINCT orphan) as orphan_claims,
           count(DISTINCT p) as total_patches,
           count(DISTINCT v) as total_verifications
"#;

/// Find all patches that touch a symbol.
/// TODO: Requires :Symbol nodes and :DEFINED_IN relationship.
/// Enable after implementing symbol graph in projector.
pub const PATCHES_BY_SYMBOL: &str = r#"
    MATCH (p:Patch)-[:MODIFIES]->(f:File)<-[:DEFINED_IN]-(sym:Symbol)
    WHERE sym.name = $symbol_name
    RETURN p.id as patch_id, 
           p.applied_at as applied_at,
           f.path as file_path
    ORDER BY p.applied_at DESC
"#;

/// Evidence chain for a claim.
pub const EVIDENCE_CHAIN: &str = r#"
    MATCH path = (c:Claim {id: $claim_id})<-[:PROVES]-(v:Verification)-[:VERIFIES]->(p:Patch)-[:MODIFIES]->(f:File)
    RETURN c.content as claim,
           v.passed as verification_passed,
           v.ts as verification_time,
           p.id as patch_id,
           collect(f.path) as files_modified
"#;
