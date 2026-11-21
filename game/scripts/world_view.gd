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

## Sprites des batiments : fiche de metadonnees par batiment (GRAPHICS-01 section 3.9).
## Les batiments sans sprite gardent l'affichage provisoire.
const SPRITE_SHEETS := {
	"four": "res://assets/sprites/ent/four-01/four-01.json",
	"foreuse": "res://assets/sprites/ent/foreuse-01/foreuse-01.json",
	"assembleur": "res://assets/sprites/ent/assembleur-01/assembleur-01.json",
}
const SPR := "res://assets/sprites/"
var _port_tex: Array = [] ## raccords de sortie N, E, S, O
var _item_tex: Array = [] ## par objet : sprite de tapis (24 px) ou null
var _res_tex: Array = [] ## par ressource : [forme][richesse] -> texture
var _nat := {} ## 1 feuillu, 2 conifere, 3 rocher -> [[texture, ombre], ...]
var _sprites := {} ## index de batiment -> {base, shadow, glow, parts, smoke, canvas, anchor, ...}
var _glow_layer: Node2D
var _fx_layer: Node2D
var _glow_jobs: Array = [] ## [texture, rect, alpha]
var _smoke_jobs: Array = [] ## [texture, rect, alpha]

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
	_load_sprites()
	invalidate_terrain()


func _ready() -> void:
	# lueurs en mode additif, puis effets aeriens (fumee) au-dessus des batiments
	_glow_layer = Node2D.new()
	var mat := CanvasItemMaterial.new()
	mat.blend_mode = CanvasItemMaterial.BLEND_MODE_ADD
	_glow_layer.material = mat
	add_child(_glow_layer)
	_glow_layer.draw.connect(_draw_glow_layer)
	_fx_layer = Node2D.new()
	add_child(_fx_layer)
	_fx_layer.draw.connect(_draw_fx_layer)


func _load_sprites() -> void:
	_sprites.clear()
	var keys: PackedStringArray = sim.proto_keys()
	for key in SPRITE_SHEETS:
		var proto := Array(keys).find(key)
		if proto < 0:
			continue
		var path: String = SPRITE_SHEETS[key]
		var meta = JSON.parse_string(FileAccess.get_file_as_string(path))
		if typeof(meta) != TYPE_DICTIONARY:
			push_warning("fiche de sprite illisible : " + path)
			continue
		var dir := path.get_base_dir()
		var sp := {
			"canvas": Vector2(meta.canvas_px[0], meta.canvas_px[1]),
			"anchor": Vector2(meta.anchor_px[0], meta.anchor_px[1]),
			"base": load(dir + "/" + meta.base),
			"shadow": load(dir + "/" + meta.shadow) if meta.has("shadow") else null,
			"glow": load(dir + "/" + meta.glow) if meta.has("glow") else null,
			"state": load(dir + "/" + meta.state) if meta.has("state") else null,
			"ports": bool(meta.get("ports", false)),
			"parts": [],
			"part_ticks": int(meta.get("part_ticks_per_frame", 6)),
			"smoke": [],
			"smoke_origin": Vector2(meta.smoke_origin_px[0], meta.smoke_origin_px[1]) if meta.has("smoke_origin_px") else Vector2(-1, -1),
		}
		for f in meta.get("part_frames", []):
			sp.parts.append(load(dir + "/" + f))
		for f in meta.get("smoke_frames", []):
			sp.smoke.append(load(f))
		_sprites[proto] = sp
	# raccords de sortie communs
	_port_tex.clear()
	for d in ["n", "e", "s", "w"]:
		_port_tex.append(_try_load(SPR + "ent/commun/ent_part-port_%s.png" % d))
	# objets : itm_<cle>_sol.png (les cles du noyau utilisent _, les fichiers -)
	_item_tex.clear()
	for k in sim.item_keys():
		_item_tex.append(_try_load(SPR + "itm/itm_%s_sol.png" % k.replace("_", "-")))
	# gisements : res_ressource-<lettre>_f<forme>_r<richesse>.png
	_res_tex.clear()
	for i in sim.resource_colors().size():
		var forms := []
		for f in 4:
			var rs := []
			for r in 4:
				rs.append(_try_load(SPR + "res/res_ressource-%s_f%d_r%d.png" % [char(97 + i), f + 1, r + 1]))
			forms.append(rs)
		_res_tex.append(forms)
	# arbres et rochers
	_nat.clear()
	for entry in [[1, "nat_arbre-feuillu", 3], [2, "nat_arbre-conifere", 2], [3, "nat_rocher-petit", 2]]:
		var lst := []
		for v in entry[2]:
			var t = _try_load(SPR + "nat/%s_v%d.png" % [entry[1], v + 1])
			if t != null:
				lst.append([t, _try_load(SPR + "nat/%s_v%d_shadow.png" % [entry[1], v + 1])])
		if not lst.is_empty():
			_nat[entry[0]] = lst


func _try_load(path: String):
	return load(path) if ResourceLoader.exists(path) else null


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
	if _glow_layer:
		_glow_layer.queue_redraw()
		_fx_layer.queue_redraw()


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
			# teinte legere : les sprites de minerai sont dessines par-dessus au zoom normal
			c = c.lerp(rc, 0.12 + 0.04 * rich)
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
	# gisements, puis arbres et rochers (au-dela d'un certain zoom)
	if zoom < 14.0:
		return
	var sc := zoom / 64.0
	var feats := []
	for cy in range(cy0, cy1 + 1):
		for cx in range(cx0, cx1 + 1):
			var data: PackedByteArray = _chunk_data.get(Vector2i(cx, cy), PackedByteArray())
			if data.is_empty():
				continue
			for i in CHUNK * CHUNK:
				var f := data[1024 + i]
				var rk := data[2048 + i]
				if f == 0 and rk == 255:
					continue
				var tx := cx * CHUNK + i % CHUNK
				var ty := cy * CHUNK + i / CHUNK
				if tx < r.position.x or ty < r.position.y or tx > r.end.x or ty > r.end.y:
					continue
				var hsh := absi((tx * 73856093) ^ (ty * 19349663))
				var rich := data[3072 + i]
				if rk != 255 and rich > 0 and rk < _res_tex.size():
					var ri: int = [0, 0, 2, 3][mini(rich, 3)]
					var tex = _res_tex[rk][hsh % 4][ri]
					if tex != null:
						draw_texture_rect(tex, Rect2(tile_to_screen(tx, ty), Vector2(zoom, zoom)), false)
				if f != 0:
					feats.append([tx, ty, f, hsh])
	for ft in feats:
		_draw_feature(ft[0], ft[1], ft[2], ft[3], sc)


func _draw_feature(tx: int, ty: int, f: int, hsh: int, sc: float) -> void:
	var c := tile_to_screen(tx + 0.5, ty + 0.5)
	if _nat.has(f):
		var lst: Array = _nat[f]
		var v: Array = lst[(hsh >> 3) % lst.size()]
		var tex: Texture2D = v[0]
		var size := Vector2(tex.get_width(), tex.get_height())
		var anchor := Vector2(size.x * 0.5, size.y - 32.0) # centre de la case, bas de l'emprise
		var rect := Rect2(c - anchor * sc, size * sc)
		if v[1] != null:
			draw_texture_rect(v[1], rect, false)
		draw_texture_rect(tex, rect, false)
		return
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
	if item < _item_tex.size() and _item_tex[item] != null:
		var sz := zoom * 24.0 / 64.0 * scale_f
		draw_texture_rect(_item_tex[item], Rect2(center - Vector2(sz, sz) * 0.5, Vector2(sz, sz)), false)
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


## Coin haut-gauche du canevas et echelle, a partir de l'ancre (centre de l'emprise).
func _sprite_frame(sp: Dictionary, x: int, y: int, w: int, h: int) -> Array:
	var sc := zoom / 64.0
	var center := tile_to_screen(x + w * 0.5, y + h * 0.5)
	var tl: Vector2 = center - sp.anchor * sc
	return [tl, sc]


func _draw_sprite_shadow(proto: int, x: int, y: int, w: int, h: int) -> void:
	var sp: Dictionary = _sprites[proto]
	if sp.shadow == null:
		return
	var f := _sprite_frame(sp, x, y, w, h)
	draw_texture_rect(sp.shadow, Rect2(f[0], sp.canvas * f[1]), false)


func _draw_sprite_entity(id: int, proto: int, x: int, y: int, w: int, h: int, d: int, status: int) -> void:
	var sp: Dictionary = _sprites[proto]
	var f := _sprite_frame(sp, x, y, w, h)
	var tl: Vector2 = f[0]
	var sc: float = f[1]
	var rect := Rect2(tl, sp.canvas * sc)
	draw_texture_rect(sp.base, rect, false)
	var working := status == 1 or status == 6
	var tick: int = sim.tick()
	if working and not sp.parts.is_empty():
		var frame: int = (tick / sp.part_ticks + id) % sp.parts.size()
		draw_texture_rect(sp.parts[frame], rect, false)
	if working and sp.glow != null:
		var pulse := 0.8 + 0.2 * sin(float(tick + id * 13) * 0.21)
		_glow_jobs.append([sp.glow, rect, pulse])
	# fumee : bouffees regulieres, calculees depuis le temps de simulation
	if working and not sp.smoke.is_empty() and sp.smoke_origin.x >= 0.0:
		var origin: Vector2 = tl + sp.smoke_origin * sc
		for k in 3:
			var phase := float((tick + k * 25 + id * 7) % 75) / 75.0
			var size := 32.0 * sc * (1.0 + phase * 0.6)
			var c := origin + Vector2(6.0 * phase, -44.0 * phase) * sc
			var tex: Texture2D = sp.smoke[mini(int(phase * sp.smoke.size()), sp.smoke.size() - 1)]
			_smoke_jobs.append([tex, Rect2(c - Vector2(size, size) * 0.5, Vector2(size, size)), (1.0 - phase) * 0.9])
	if sp.state != null:
		draw_texture_rect(sp.state, rect, false, STATUS_COLORS[status].lightened(0.15))
	else:
		draw_circle(tl + Vector2(sp.canvas.x - 10.0, sp.canvas.y - 10.0) * sc, maxf(3.0, zoom * 0.09), STATUS_COLORS[status])
	if sp.ports and _port_tex.size() == 4 and _port_tex[d] != null:
		var edge: Vector2
		match d:
			0: edge = Vector2(x + 0.5, y)
			1: edge = Vector2(x + w, y + 0.5)
			2: edge = Vector2(x + w - 0.5, y + h)
			_: edge = Vector2(x, y + h - 0.5)
		var pc := tile_to_screen(edge.x, edge.y)
		var ps := 32.0 * sc
		draw_texture_rect(_port_tex[d], Rect2(pc - Vector2(ps, ps) * 0.5, Vector2(ps, ps)), false)
	if id == selected_id:
		draw_rect(Rect2(tile_to_screen(x, y), Vector2(w, h) * zoom), Color(1, 1, 1, 0.9), false, maxf(2.0, zoom * 0.06))


func _draw_glow_layer() -> void:
	for j in _glow_jobs:
		_glow_layer.draw_texture_rect(j[0], j[1], false, Color(1, 1, 1, j[2]))


func _draw_fx_layer() -> void:
	for j in _smoke_jobs:
		_fx_layer.draw_texture_rect(j[0], j[1], false, Color(1, 1, 1, j[2]))


func _draw_entity(id: int, proto: int, x: int, y: int, w: int, h: int, d: int, status: int, e1: int, e2: int) -> void:
	if _sprites.has(proto):
		_draw_sprite_entity(id, proto, x, y, w, h, d, status)
		return
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
	# 3. ombres des batiments a sprite, puis batiments
	_glow_jobs.clear()
	_smoke_jobs.clear()
	for k in n:
		var b := k * 10
		if _sprites.has(ents[b + 1]):
			_draw_sprite_shadow(ents[b + 1], ents[b + 2], ents[b + 3], ents[b + 4], ents[b + 5])
	# tri par bord bas de l'emprise : un batiment haut passe devant ce qui est derriere lui
	var order := range(n)
	order.sort_custom(func(a: int, c: int) -> bool:
		var ya := ents[a * 10 + 3] + ents[a * 10 + 5]
		var yc := ents[c * 10 + 3] + ents[c * 10 + 5]
		return ya < yc or (ya == yc and ents[a * 10 + 2] < ents[c * 10 + 2]))
	for k in order:
		var b: int = k * 10
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
