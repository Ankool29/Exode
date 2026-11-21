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
	"coffre": "res://assets/sprites/sto/coffre-01/coffre-01.json",
	"benne": "res://assets/sprites/wst/benne-01/benne-01.json",
	"poteau": "res://assets/sprites/net/poteau-01/poteau-01.json",
	"generateur": "res://assets/sprites/net/generateur-01/generateur-01.json",
}
## Terrain detaille (a partir de ZOOM_TERRAIN px par case) : grandes textures 256 px
## echantillonnees selon la position, rives par masque de cotes N=1, E=2, S=4, W=8.
const ZOOM_TERRAIN := 16.0
var _ter := {} ## herbe, terre : texture ; eau : [4 images]
var _trans := {} ## "eau" / "terre" -> {"mask": [16], "coin": [ne, se, sw, nw]}
var _decals: Array = []
var _belt_tex := {} ## h, v, courbe-ne..., fin-n...
var _tile_cache := {} ## Vector2i -> PackedInt32Array (code de rendu par case)
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
			"status_dot": proto_kinds[proto] in ["drill", "machine"],
			"wire": Vector2(meta.wire_px[0], meta.wire_px[1]) if meta.has("wire_px") else Vector2(-1, -1),
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
	# terrain, rives, convoyeurs
	_ter.clear()
	_trans.clear()
	_decals.clear()
	_belt_tex.clear()
	var herbe = _try_load(SPR + "ter/ter_herbe_grand.png")
	var terre = _try_load(SPR + "ter/ter_terre_grand.png")
	var eau := []
	for i in 4:
		var t = _try_load(SPR + "ter/ter_eau_grand_frame%d.png" % i)
		if t != null:
			eau.append(t)
	if herbe != null and terre != null and eau.size() == 4:
		_ter = {"herbe": herbe, "terre": terre, "eau": eau}
		for pair in ["eau", "terre"]:
			var masks := []
			for m in 16:
				masks.append(_try_load(SPR + "ter/ter_trans-herbe-%s_%02d.png" % [pair, m]))
			var coins := []
			for c in ["ne", "se", "sw", "nw"]:
				coins.append(_try_load(SPR + "ter/ter_trans-herbe-%s_coin-%s.png" % [pair, c]))
			_trans[pair] = {"mask": masks, "coin": coins}
		for i in 4:
			var d = _try_load(SPR + "ter/ter_decal_v%d.png" % (i + 1))
			if d != null:
				_decals.append(d)
	for k in ["h", "v", "courbe-ne", "courbe-nw", "courbe-se", "courbe-sw", "fin-n", "fin-e", "fin-s", "fin-w"]:
		var t = _try_load(SPR + "blt/blt_tapis-1_%s.png" % k)
		if t != null:
			_belt_tex[k] = t
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


## full : le sol a change (remblai). Sinon seuls les gisements (epuisement) sont relus.
func invalidate_terrain(full := true) -> void:
	_chunk_tex.clear()
	_chunk_data.clear()
	if full:
		_tile_cache.clear()


func _process(delta: float) -> void:
	# les gisements s'epuisent : rafraichissement periodique des chunks visibles
	_refresh_timer += delta
	if _refresh_timer > 3.0:
		_refresh_timer = 0.0
		invalidate_terrain(false)
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
	var data: PackedByteArray = _chunk_bytes(key)
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


func _chunk_bytes(key: Vector2i) -> PackedByteArray:
	if not _chunk_data.has(key):
		_chunk_data[key] = sim.chunk_data(key.x, key.y)
	return _chunk_data[key]


func _ground_at(tx: int, ty: int) -> int:
	var key := Vector2i(floori(float(tx) / CHUNK), floori(float(ty) / CHUNK))
	var data := _chunk_bytes(key)
	return data[(ty - key.y * CHUNK) * CHUNK + (tx - key.x * CHUNK)]


## Code de rendu d'une case : genre * 256 + indice, plus decal * 65536.
## Genres : 0 herbe, 1 terre, 2 eau, 3 sable, 4 remblai, 5 rive eau (masque), 6 coin eau,
## 7 rive terre (masque), 8 coin terre.
func _tile_codes(key: Vector2i) -> PackedInt32Array:
	if _tile_cache.has(key):
		return _tile_cache[key]
	var codes := PackedInt32Array()
	codes.resize(CHUNK * CHUNK)
	for i in CHUNK * CHUNK:
		var tx := key.x * CHUNK + i % CHUNK
		var ty := key.y * CHUNK + i / CHUNK
		var g := _ground_at(tx, ty)
		var code := 0
		match g:
			1: code = 1 << 8
			2: code = 3 << 8
			3: code = 2 << 8
			4: code = 4 << 8
			_:
				code = _edge_code(tx, ty, 3, 5)
				if code == 0:
					code = _edge_code(tx, ty, -1, 7)
				if code == 0:
					var h := absi((tx * 73856093) ^ (ty * 19349663))
					if h % 7 == 0 and not _decals.is_empty():
						code = ((h >> 4) % _decals.size() + 1) << 16
		codes[i] = code
	_tile_cache[key] = codes
	return codes


## Rive autour d'une case d'herbe. kind = 3 : eau ; -1 : terre, sable ou remblai.
func _edge_code(tx: int, ty: int, kind: int, base: int) -> int:
	var is_k := func(g: int) -> bool: return g == 3 if kind == 3 else (g == 1 or g == 2 or g == 4)
	var m := 0
	if is_k.call(_ground_at(tx, ty - 1)): m |= 1
	if is_k.call(_ground_at(tx + 1, ty)): m |= 2
	if is_k.call(_ground_at(tx, ty + 1)): m |= 4
	if is_k.call(_ground_at(tx - 1, ty)): m |= 8
	if m != 0:
		return (base << 8) | m
	var corners := [[1, -1], [1, 1], [-1, 1], [-1, -1]] # ne, se, sw, nw
	for c in 4:
		if is_k.call(_ground_at(tx + corners[c][0], ty + corners[c][1])):
			return ((base + 1) << 8) | c
	return 0


func _draw_terrain_detailed(r: Rect2i) -> void:
	var frame: int = (sim.tick() / 8) % 4
	var cell := Vector2(zoom + 0.5, zoom + 0.5) # leger recouvrement : pas de joint visible
	var sand_mod := Color(1.0, 0.92, 0.68)
	var fill_mod := Color(0.78, 0.76, 0.74)
	for ty in range(r.position.y, r.end.y + 1):
		for tx in range(r.position.x, r.end.x + 1):
			var key := Vector2i(floori(float(tx) / CHUNK), floori(float(ty) / CHUNK))
			var code: int = _tile_codes(key)[(ty - key.y * CHUNK) * CHUNK + (tx - key.x * CHUNK)]
			var kind := (code >> 8) & 255
			var idx := code & 255
			var rect := Rect2(tile_to_screen(tx, ty), cell)
			var src := Rect2(posmod(tx, 4) * 64, posmod(ty, 4) * 64, 64, 64)
			match kind:
				0: draw_texture_rect_region(_ter.herbe, rect, src)
				1: draw_texture_rect_region(_ter.terre, rect, src)
				2: draw_texture_rect_region(_ter.eau[frame], rect, src)
				3: draw_texture_rect_region(_ter.terre, rect, src, sand_mod)
				4: draw_texture_rect_region(_ter.terre, rect, src, fill_mod)
				5: _draw_tex_or(_trans.eau.mask[idx], rect, _ter.herbe, src)
				6: _draw_tex_or(_trans.eau.coin[idx], rect, _ter.herbe, src)
				7: _draw_tex_or(_trans.terre.mask[idx], rect, _ter.herbe, src)
				8: _draw_tex_or(_trans.terre.coin[idx], rect, _ter.herbe, src)
			var dec := code >> 16
			if dec > 0:
				var off := Vector2(float((tx * 37 + ty * 11) & 31), float((tx * 13 + ty * 29) & 31)) * zoom / 64.0
				draw_texture_rect(_decals[dec - 1], Rect2(rect.position + off, Vector2(zoom, zoom) * 0.5), false)


func _draw_tex_or(tex, rect: Rect2, fallback: Texture2D, src: Rect2) -> void:
	if tex != null:
		draw_texture_rect(tex, rect, false)
	else:
		draw_texture_rect_region(fallback, rect, src)


func _draw_terrain(r: Rect2i) -> void:
	var cx0 := floori(float(r.position.x) / CHUNK)
	var cy0 := floori(float(r.position.y) / CHUNK)
	var cx1 := floori(float(r.end.x) / CHUNK)
	var cy1 := floori(float(r.end.y) / CHUNK)
	if zoom >= ZOOM_TERRAIN and not _ter.is_empty():
		_draw_terrain_detailed(r)
	else:
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


const DIR_LETTER := ["n", "e", "s", "w"]


## Piece de tapis : droite, virage (cotes relies) ou extremite (pas de convoyeur devant).
func _belt_piece(x: int, y: int, d: int, curve: int, belts: Dictionary) -> String:
	if curve != 0:
		var side := (d + 3) % 4 if curve == 1 else (d + 1) % 4
		var a: String = DIR_LETTER[side]
		var b: String = DIR_LETTER[d]
		# nom : n ou s d'abord, puis e ou w
		return "courbe-" + (a + b if a in ["n", "s"] else b + a)
	var v := _dir_vec(d)
	if not belts.has(Vector2i(x + int(v.x), y + int(v.y))):
		return "fin-" + DIR_LETTER[d]
	return "v" if d % 2 == 0 else "h"


func _draw_belt(x: int, y: int, d: int, curve: int, belts := {}) -> void:
	var p := tile_to_screen(x, y)
	if not _belt_tex.is_empty():
		var piece := _belt_piece(x, y, d, curve, belts)
		if _belt_tex.has(piece):
			draw_texture_rect(_belt_tex[piece], Rect2(p, Vector2(zoom + 0.5, zoom + 0.5)), false)
			if curve == 0:
				_draw_chevrons(p, d)
			return
	var rect := Rect2(p + Vector2(1, 1), Vector2(zoom - 2, zoom - 2))
	draw_rect(rect, Color("34373b"))
	draw_rect(rect, Color("1c1d1f"), false, 1.0)
	var c := p + Vector2(zoom, zoom) * 0.5
	var v := _dir_vec(d)
	var side := Vector2(-v.y, v.x)
	# separation des deux voies
	draw_line(c - v * zoom * 0.45, c + v * zoom * 0.45, Color(0.1, 0.1, 0.1, 0.6), 1.0)
	_draw_chevrons(p, d)
	if curve != 0:
		draw_circle(c, zoom * 0.07, Color(0.85, 0.65, 0.2, 0.6))


## Chevrons du sens de marche, animes par le temps de simulation.
func _draw_chevrons(p: Vector2, d: int) -> void:
	var c := p + Vector2(zoom, zoom) * 0.5
	var v := _dir_vec(d)
	var side := Vector2(-v.y, v.x)
	var t := fmod(float(sim.tick()) * 16.0 / 256.0, 1.0)
	for k in 2:
		var o := (float(k) * 0.5 + t * 0.5) - 0.5
		var tip := c + v * zoom * (o + 0.1)
		var col := Color(0.85, 0.65, 0.2, 0.8)
		for lane_s: float in [-1.0, 1.0]:
			var lc := tip + side * zoom * LANE_OFF * lane_s
			draw_line(lc - v * zoom * 0.08 + side * zoom * 0.06, lc, col, maxf(1.0, zoom * 0.04))
			draw_line(lc - v * zoom * 0.08 - side * zoom * 0.06, lc, col, maxf(1.0, zoom * 0.04))


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
	elif sp.status_dot:
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
	var belts := {}
	for k in n:
		var b := k * 10
		if proto_kinds[ents[b + 1]] == "belt":
			belts[Vector2i(ents[b + 2], ents[b + 3])] = true
	for k in n:
		var b := k * 10
		if proto_kinds[ents[b + 1]] == "belt":
			_draw_belt(ents[b + 2], ents[b + 3], ents[b + 6], ents[b + 8], belts)
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
	var wire_off := Vector2.ZERO
	var pole_proto := Array(proto_kinds).find("pole")
	if _sprites.has(pole_proto) and _sprites[pole_proto].wire.x >= 0.0:
		wire_off = (_sprites[pole_proto].wire - _sprites[pole_proto].anchor) * (zoom / 64.0)
	for k in links.size() / 4:
		var a := tile_to_screen(links[k * 4] + 0.5, links[k * 4 + 1] + 0.5) + wire_off
		var bb := tile_to_screen(links[k * 4 + 2] + 0.5, links[k * 4 + 3] + 0.5) + wire_off
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
