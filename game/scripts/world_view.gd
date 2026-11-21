## Affichage PROVISOIRE du monde : formes simples en attendant les sprites (GRAPHICS-01).
## Tout est dessine en coordonnees d'ecran calculees depuis une camera en flottants 64 bits
## (origine flottante) : pas de tremblement loin de l'origine du monde.
extends Node2D

const CHUNK := 32
const LANE_OFF := 12.0 / 64.0

## Etats (meme ordre que le noyau)
const STATUS_COLORS := [
	Color(0.55, 0.55, 0.55), # inactif
	Color(0.35, 0.85, 0.35), # en marche
	Color(0.95, 0.80, 0.25), # entree manquante
	Color(0.95, 0.55, 0.20), # sortie pleine
	Color(0.75, 0.55, 0.15), # dechets pleins
	Color(0.90, 0.20, 0.20), # pas d'energie
	Color(0.95, 0.65, 0.25), # energie faible
	Color(0.60, 0.45, 0.85), # pas de recette
	Color(0.50, 0.50, 0.70), # pas de ressource
	Color(0.55, 0.55, 0.55), # pas de cible
]
const GROUND_COLORS := [
	Color("4f7a3a"), # herbe
	Color("7a5f3e"), # terre
	Color("c9b47a"), # sable
	Color("2e5f8a"), # eau
	Color("6b5a45"), # remblai
]

var sim # ExodeSim
var cam_x := 0.0 ## camera, en cases (flottants 64 bits)
var cam_y := 0.0
var zoom := 40.0 ## pixels (unites de canevas) par case

## Construction : fantome
var ghost_proto := -1
var ghost_x := 0
var ghost_y := 0
var ghost_dir := 1
var ghost_ok := false
var ghost_size := Vector2i(1, 1)
var ghost_radius := 0
## Case marquee (demolition, defrichement, remblai, inspection)
var marker := Vector2i(0, 0)
var marker_on := false
var marker_color := Color(1, 1, 1)
var selected_id := -1

var proto_kinds: PackedStringArray
var proto_names: PackedStringArray
var item_colors: Array[Color] = []
var item_waste: PackedInt32Array
var resource_colors: Array[Color] = []

var _chunk_tex := {} ## Vector2i -> ImageTexture
var _chunk_data := {} ## Vector2i -> PackedByteArray
var _refresh_timer := 0.0
var _font: Font


func setup(s) -> void:
	sim = s
	proto_kinds = sim.proto_kinds()
	proto_names = sim.proto_names()
	item_colors.clear()
	for c in sim.item_colors():
		item_colors.append(Color(c))
	item_waste = sim.item_is_waste()
	resource_colors.clear()
	for c in sim.resource_colors():
		resource_colors.append(Color(c))
	_font = ThemeDB.fallback_font
	invalidate_terrain()


func invalidate_terrain() -> void:
	_chunk_tex.clear()
	_chunk_data.clear()


func _process(delta: float) -> void:
	# les gisements s'epuisent : rafraichissement periodique des chunks visibles
	_refresh_timer += delta
	if _refresh_timer > 3.0:
		_refresh_timer = 0.0
		invalidate_terrain()
	queue_redraw()


# ---------------------------------------------------------------- reperes

func view_size() -> Vector2:
	return get_viewport_rect().size


func tile_to_screen(tx: float, ty: float) -> Vector2:
	var s := view_size()
	return Vector2((tx - cam_x) * zoom + s.x * 0.5, (ty - cam_y) * zoom + s.y * 0.5)


func screen_to_tile(p: Vector2) -> Vector2i:
	var s := view_size()
	return Vector2i(floori(cam_x + (p.x - s.x * 0.5) / zoom), floori(cam_y + (p.y - s.y * 0.5) / zoom))


func visible_rect() -> Rect2i:
	var s := view_size()
	var x0 := floori(cam_x - s.x * 0.5 / zoom) - 1
	var y0 := floori(cam_y - s.y * 0.5 / zoom) - 1
	var x1 := ceili(cam_x + s.x * 0.5 / zoom) + 1
	var y1 := ceili(cam_y + s.y * 0.5 / zoom) + 1
	return Rect2i(x0, y0, x1 - x0, y1 - y0)


# ---------------------------------------------------------------- terrain

func _chunk_texture(key: Vector2i) -> ImageTexture:
	if _chunk_tex.has(key):
		return _chunk_tex[key]
	var data: PackedByteArray = sim.chunk_data(key.x, key.y)
	_chunk_data[key] = data
	var img := Image.create(CHUNK, CHUNK, false, Image.FORMAT_RGBA8)
	for i in CHUNK * CHUNK:
		var g := data[i]
		var c: Color = GROUND_COLORS[mini(g, GROUND_COLORS.size() - 1)]
		var lx := i % CHUNK
		var ly := i / CHUNK
		# legere variation pour casser l'uniformite
		var h := ((key.x * CHUNK + lx) * 73856093) ^ ((key.y * CHUNK + ly) * 19349663)
		c = c.lightened(float(h & 7) * 0.012)
		var rk := data[2048 + i]
		var rich := data[3072 + i]
		if rk != 255 and rich > 0 and rk < resource_colors.size():
			var rc: Color = resource_colors[rk]
			var dot := ((lx + ly * 3 + (h >> 4)) % 4) < rich
			c = c.lerp(rc, 0.45 + 0.15 * rich) if dot else c.lerp(rc, 0.25)
		img.set_pixel(lx, ly, c)
	var tex := ImageTexture.create_from_image(img)
	_chunk_tex[key] = tex
	return tex


func _draw_terrain(r: Rect2i) -> void:
	var cx0 := floori(float(r.position.x) / CHUNK)
	var cy0 := floori(float(r.position.y) / CHUNK)
	var cx1 := floori(float(r.end.x) / CHUNK)
	var cy1 := floori(float(r.end.y) / CHUNK)
	for cy in range(cy0, cy1 + 1):
		for cx in range(cx0, cx1 + 1):
			var key := Vector2i(cx, cy)
			var tex := _chunk_texture(key)
			var p := tile_to_screen(cx * CHUNK, cy * CHUNK)
			draw_texture_rect(tex, Rect2(p, Vector2(CHUNK * zoom, CHUNK * zoom)), false)
	# arbres et rochers (au-dela d'un certain zoom)
	if zoom < 14.0:
		return
	for cy in range(cy0, cy1 + 1):
		for cx in range(cx0, cx1 + 1):
			var data: PackedByteArray = _chunk_data.get(Vector2i(cx, cy), PackedByteArray())
			if data.is_empty():
				continue
			for i in CHUNK * CHUNK:
				var f := data[1024 + i]
				if f == 0:
					continue
				var tx := cx * CHUNK + i % CHUNK
				var ty := cy * CHUNK + i / CHUNK
				if tx < r.position.x or ty < r.position.y or tx > r.end.x or ty > r.end.y:
					continue
				var c := tile_to_screen(tx + 0.5, ty + 0.5)
				match f:
					1:
						draw_circle(c + Vector2(zoom * 0.08, zoom * 0.1), zoom * 0.42, Color(0, 0, 0, 0.25))
						draw_circle(c, zoom * 0.42, Color("2f5a26"))
						draw_circle(c - Vector2(zoom * 0.1, zoom * 0.1), zoom * 0.2, Color("4d7d33"))
					2:
						var pts := PackedVector2Array([c + Vector2(0, -zoom * 0.48), c + Vector2(zoom * 0.38, zoom * 0.35), c + Vector2(-zoom * 0.38, zoom * 0.35)])
						draw_colored_polygon(pts, Color("24482a"))
					3:
						var pts := PackedVector2Array([c + Vector2(-zoom * 0.35, zoom * 0.2), c + Vector2(-zoom * 0.15, -zoom * 0.3), c + Vector2(zoom * 0.3, -zoom * 0.2), c + Vector2(zoom * 0.38, zoom * 0.25)])
						draw_colored_polygon(pts, Color("8a8a85"))


# ---------------------------------------------------------------- entites

func _dir_vec(d: int) -> Vector2:
	match d:
		0: return Vector2(0, -1)
		1: return Vector2(1, 0)
		2: return Vector2(0, 1)
	return Vector2(-1, 0)


func _kind_color(kind: String, proto: int) -> Color:
	match kind:
		"drill": return Color("8b6d3a")
		"machine":
			var n := proto_names[proto]
			if n.begins_with("Four"):
				return Color("5a4a44")
			if n.begins_with("Recycl"):
				return Color("4f5a3c")
			return Color("4c5866")
		"chest": return Color("7a5a33")
		"waste_bin": return Color("6b6a3c")
		"generator": return Color("3e4a57")
		"pole": return Color("5a4632")
	return Color(0.4, 0.4, 0.4)


func _draw_item(center: Vector2, item: int, scale_f := 1.0) -> void:
	if item < 0 or item >= item_colors.size():
		return
	var s := zoom * 0.30 * scale_f
	var rect := Rect2(center - Vector2(s, s) * 0.5, Vector2(s, s))
	draw_rect(rect, item_colors[item])
	if item_waste[item] == 1:
		draw_line(rect.position, rect.end, Color(0.95, 0.75, 0.1), maxf(1.0, s * 0.15))
	draw_rect(rect, Color(0, 0, 0, 0.6), false, 1.0)


func _draw_belt(x: int, y: int, d: int, curve: int) -> void:
	var p := tile_to_screen(x, y)
	var rect := Rect2(p + Vector2(1, 1), Vector2(zoom - 2, zoom - 2))
	draw_rect(rect, Color("34373b"))
	draw_rect(rect, Color("1c1d1f"), false, 1.0)
	var c := p + Vector2(zoom, zoom) * 0.5
	var v := _dir_vec(d)
	var side := Vector2(-v.y, v.x)
	# separation des deux voies
	draw_line(c - v * zoom * 0.45, c + v * zoom * 0.45, Color(0.1, 0.1, 0.1, 0.6), 1.0)
	# chevrons (sens de marche) ; animes par le temps de simulation
	var t := fmod(float(sim.tick()) * 16.0 / 256.0, 1.0)
	for k in 2:
		var o := (float(k) * 0.5 + t * 0.5) - 0.5
		var tip := c + v * zoom * (o + 0.1)
		var col := Color(0.85, 0.65, 0.2, 0.8)
		for lane_s: float in [-1.0, 1.0]:
			var lc := tip + side * zoom * LANE_OFF * lane_s
			draw_line(lc - v * zoom * 0.08 + side * zoom * 0.06, lc, col, maxf(1.0, zoom * 0.04))
			draw_line(lc - v * zoom * 0.08 - side * zoom * 0.06, lc, col, maxf(1.0, zoom * 0.04))
	if curve != 0:
		draw_circle(c, zoom * 0.07, Color(0.85, 0.65, 0.2, 0.6))


func _draw_entity(id: int, proto: int, x: int, y: int, w: int, h: int, d: int, status: int, e1: int, e2: int) -> void:
	var kind := proto_kinds[proto]
	var p := tile_to_screen(x, y)
	var size := Vector2(w, h) * zoom
	match kind:
		"belt":
			pass # dessines avant les objets
		"inserter":
			var c := p + size * 0.5
			draw_rect(Rect2(c - Vector2(zoom, zoom) * 0.2, Vector2(zoom, zoom) * 0.4), Color("3a3d42"))
			var v := _dir_vec(d)
			var pick := c - v * zoom * 0.75
			var drop := c + v * zoom * 0.75
			var prog := absf(float(e2)) / 1000.0
			var hand := pick.lerp(drop, prog) if e2 >= 0 else drop.lerp(pick, prog)
			draw_line(c, hand, Color("c99a2e"), maxf(2.0, zoom * 0.1))
			draw_circle(hand, zoom * 0.09, Color("e0b23a"))
			if e1 >= 0:
				_draw_item(hand, e1, 0.8)
			draw_circle(c + Vector2(zoom * 0.3, -zoom * 0.3), zoom * 0.07, STATUS_COLORS[status])
		"pole":
			var c := p + size * 0.5
			draw_circle(c + Vector2(zoom * 0.15, zoom * 0.15), zoom * 0.2, Color(0, 0, 0, 0.25))
			draw_rect(Rect2(c - Vector2(zoom, zoom) * 0.12, Vector2(zoom, zoom) * 0.24), Color("5a4632"))
			draw_circle(c, zoom * 0.06, Color("c87533"))
		_:
			var col := _kind_color(kind, proto)
			draw_rect(Rect2(p + Vector2(zoom * 0.12, zoom * 0.12), size), Color(0, 0, 0, 0.3))
			draw_rect(Rect2(p + Vector2(2, 2), size - Vector2(4, 4)), col)
			draw_rect(Rect2(p + Vector2(2, 2), size - Vector2(4, 4)), Color(0.12, 0.12, 0.12), false, maxf(1.0, zoom * 0.05))
			if zoom >= 18.0:
				var label := proto_names[proto]
				var fs := int(clampf(zoom * 0.28, 8.0, 22.0))
				draw_string(_font, p + Vector2(zoom * 0.12, zoom * 0.12 + fs), label, HORIZONTAL_ALIGNMENT_LEFT, size.x - zoom * 0.2, fs, Color(0.92, 0.92, 0.92))
			if kind == "machine" or kind == "drill":
				# barre d'avancement
				var bar := Rect2(p + Vector2(zoom * 0.15, size.y - zoom * 0.28), Vector2(size.x - zoom * 0.3, zoom * 0.12))
				draw_rect(bar, Color(0, 0, 0, 0.5))
				draw_rect(Rect2(bar.position, Vector2(bar.size.x * float(e2) / 1000.0, bar.size.y)), Color("e0b23a"))
				# sortie
				var v := _dir_vec(d)
				var cc := p + size * 0.5
				draw_line(cc, cc + v * (size.x * 0.45), Color(0.9, 0.9, 0.9, 0.5), maxf(1.0, zoom * 0.05))
			if kind == "generator":
				draw_circle(p + size * 0.5, zoom * 0.5, Color("c87533").darkened(0.3))
			if kind != "chest":
				draw_circle(p + Vector2(size.x - zoom * 0.25, zoom * 0.25), zoom * 0.12, STATUS_COLORS[status])
	if id == selected_id:
		draw_rect(Rect2(p, size), Color(1, 1, 1, 0.9), false, maxf(2.0, zoom * 0.06))


func _draw() -> void:
	if sim == null:
		return
	var r := visible_rect()
	_draw_terrain(r)
	var ents: PackedInt32Array = sim.entities_in_rect(r.position.x, r.position.y, r.end.x, r.end.y)
	var n := ents.size() / 10
	# 1. convoyeurs
	for k in n:
		var b := k * 10
		if proto_kinds[ents[b + 1]] == "belt":
			_draw_belt(ents[b + 2], ents[b + 3], ents[b + 6], ents[b + 8])
	# 2. objets sur les convoyeurs (positions relatives a une origine proche de la camera)
	var ox := floori(cam_x)
	var oy := floori(cam_y)
	var items: PackedFloat32Array = sim.belt_items_in_rect(r.position.x, r.position.y, r.end.x, r.end.y, ox, oy)
	for k in items.size() / 3:
		var c := tile_to_screen(ox + items[k * 3], oy + items[k * 3 + 1])
		_draw_item(c, int(items[k * 3 + 2]))
	# 3. batiments
	for k in n:
		var b := k * 10
		_draw_entity(ents[b], ents[b + 1], ents[b + 2], ents[b + 3], ents[b + 4], ents[b + 5], ents[b + 6], ents[b + 7], ents[b + 8], ents[b + 9])
	# 4. cables
	var links: PackedInt32Array = sim.pole_links_in_rect(r.position.x - 10, r.position.y - 10, r.end.x + 10, r.end.y + 10)
	for k in links.size() / 4:
		var a := tile_to_screen(links[k * 4] + 0.5, links[k * 4 + 1] + 0.5)
		var bb := tile_to_screen(links[k * 4 + 2] + 0.5, links[k * 4 + 3] + 0.5)
		draw_line(a, bb, Color("c87533"), maxf(1.0, zoom * 0.04))
	# 5. fantome de construction et marqueur
	if ghost_proto >= 0:
		var p := tile_to_screen(ghost_x, ghost_y)
		var size := Vector2(ghost_size) * zoom
		var col := Color(0.3, 0.9, 0.4, 0.45) if ghost_ok else Color(0.95, 0.25, 0.2, 0.45)
		if ghost_radius > 0:
			var rp := tile_to_screen(ghost_x - ghost_radius, ghost_y - ghost_radius)
			draw_rect(Rect2(rp, Vector2.ONE * (ghost_radius * 2 + 1) * zoom), Color(0.3, 0.6, 1.0, 0.12))
		draw_rect(Rect2(p, size), col)
		draw_rect(Rect2(p, size), col.lightened(0.4), false, 2.0)
		var cc := p + size * 0.5
		var v := _dir_vec(ghost_dir)
		draw_line(cc, cc + v * zoom * 0.6 * maxf(ghost_size.x, ghost_size.y) * 0.6, Color.WHITE, maxf(2.0, zoom * 0.08))
	if marker_on:
		var p := tile_to_screen(marker.x, marker.y)
		draw_rect(Rect2(p, Vector2(zoom, zoom)), marker_color, false, maxf(2.0, zoom * 0.08))
