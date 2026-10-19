import type { NodeId, TreeNode } from '~/generated'

/**
 * The visible tree from `nodes.tree`, indexed for the picker, the path display and move
 * targets. Placeholders (ancestors the caller cannot read) keep their place but carry no name.
 */
export interface TreeIndex {
  /** Depth first, siblings in order, as the server sends it. */
  order: TreeNode[]
  byId: Map<NodeId, TreeNode>
  children: Map<NodeId, TreeNode[]>
}

export const isReadable = (node: TreeNode): boolean => node.role !== undefined

export function indexTree(nodes: TreeNode[]): TreeIndex {
  const byId = new Map(nodes.map((node) => [node.id, node]))
  const children = new Map<NodeId, TreeNode[]>()
  for (const node of nodes) {
    if (node.parent === undefined) continue
    const siblings = children.get(node.parent) ?? []
    siblings.push(node)
    children.set(node.parent, siblings)
  }
  for (const siblings of children.values()) siblings.sort((a, b) => a.position - b.position)
  return { order: nodes, byId, children }
}

/** The node and its ancestors, root first. Empty when the node is not in the tree. */
export function pathTo(index: TreeIndex, id: NodeId): TreeNode[] {
  const path: TreeNode[] = []
  for (let node = index.byId.get(id); node; node = node.parent ? index.byId.get(node.parent) : undefined) {
    path.unshift(node)
  }
  return path
}

/** Every node below `id`. */
export function descendantsOf(index: TreeIndex, id: NodeId): TreeNode[] {
  const below = [...(index.children.get(id) ?? [])]
  for (let at = 0; at < below.length; at += 1) {
    const node = below[at]
    if (node) below.push(...(index.children.get(node.id) ?? []))
  }
  return below
}

/** Levels below `id`: 0 for a leaf. */
export function heightOf(index: TreeIndex, id: NodeId): number {
  const root = index.byId.get(id)
  if (!root) return 0
  return Math.max(0, ...descendantsOf(index, id).map((node) => node.depth - root.depth))
}

/** Where the app opens: the first readable node, which is the highest the caller holds. */
export const firstReadable = (index: TreeIndex): TreeNode | null => index.order.find(isReadable) ?? null

const fold = (text: string) => text.normalize('NFD').replace(/\p{Diacritic}/gu, '').toLowerCase()

export interface PickerRow {
  node: TreeNode
  readable: boolean
  /** Indentation level; 0 for every search result, which shows its trail instead. */
  indent: number
  /** Ancestors, root first, shown with search results. */
  trail: TreeNode[]
}

/**
 * The picker's rows. Without a query: the whole tree in order, placeholders included so the
 * structure reads right. With one: readable nodes whose name or a readable ancestor's name
 * contains it (case and accents ignored), so typing a client shows the client and its subtree.
 */
export function pickerRows(index: TreeIndex, query: string): PickerRow[] {
  const needle = fold(query.trim())
  if (!needle) {
    return index.order.map((node) => ({ node, readable: isReadable(node), indent: node.depth, trail: [] }))
  }
  return index.order.flatMap((node) => {
    if (!isReadable(node)) return []
    const path = pathTo(index, node.id)
    const hit = path.some((entry) => entry.name !== undefined && fold(entry.name).includes(needle))
    return hit ? [{ node, readable: true, indent: 0, trail: path.slice(0, -1) }] : []
  })
}

/** Why a node cannot take `moving` as a child, or `null` when it can. */
export type MoveRefusal = 'hidden' | 'inside' | 'current' | 'capability' | 'depth'

/**
 * Valid new parents for `moving`: readable, `node.write`, not the node or below it, not its
 * current parent, and deep enough room for its subtree (`maxDepth` levels below the root).
 */
export function moveRefusal(index: TreeIndex, moving: NodeId, target: TreeNode, maxDepth: number): MoveRefusal | null {
  const node = index.byId.get(moving)
  if (!isReadable(target)) return 'hidden'
  if (target.id === moving || pathTo(index, target.id).some((entry) => entry.id === moving)) return 'inside'
  if (node?.parent === target.id) return 'current'
  if (!target.capabilities.includes('node.write')) return 'capability'
  if (target.depth + 1 + heightOf(index, moving) > maxDepth) return 'depth'
  return null
}
