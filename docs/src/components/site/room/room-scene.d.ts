type Vec3 = [number, number, number]

/** One transformed group of primitives, with a local bounding box. */
export interface RoomGroup {
  name: string
  start: number
  end: number
  min: Vec3
  max: Vec3
  pos: Vec3
  rotY: number
}

/** A rectangle: a corner and two edge vectors. */
export interface RoomRect {
  c0: Vec3
  e1: Vec3
  e2: Vec3
  n: Vec3
}

export interface RoomScene {
  /** The primitives as an RGBA float texture's data, five texels each. */
  pack(): {
    data: Float32Array
    count: number
    width: number
    groups: RoomGroup[]
  }
  MAT: Record<string, number>
  /** Candle bulb centres, in the candle group's space. */
  bulbs: Vec3[]
  lamp: RoomRect
  window: RoomRect
  /**
   * One code per leaf of the Monstera, in order, for its leaf maps: maturity
   * in 64ths + 64 * freshness in 16ths + 1024 if the leaf is torn.
   */
  leafMats: number[]
}

export function buildRoomScene(): RoomScene
