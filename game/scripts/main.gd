## EXODE - scene principale : boucle de simulation, entrees tactiles, interface.
## Premiere version de developpement : graphismes provisoires, construction sans cout
## (la construction logistique avec materiaux viendra avec les robots, v0.4 6.3).
extends Node

const WorldView := preload("res://scripts/world_view.gd")

const AUTOSAVE := "user://exode_autosave.sav"
const AUTOSAVE_TMP := "user://exode_autosave.tmp"
const AUTOSAVE_PERIOD := 60.0
## Au-dela, la simulation ralentit au lieu de changer les regles (v0.4 section 8).
const MAX_TICKS_PER_FRAME := 4
const ZOOM_MIN := 6.0
const ZOOM_MAX := 160.0
const TAP_SLOP := 10.0

const STATUS_TEXT := [
	"Inactif", "En marche", "Attend des entrees", "Sortie pleine", "Dechets pleins",
	"Pas d'energie", "Energie insuffisante", "Pas de recette", "Gisement epuise", "Pas de cible",
]

var sim
var view: WorldView
var paused := false
var _acc := 0.0
var _autosave_timer := 0.0
var _ui_timer := 0.0

# outils
var tools: Array = [] ## [{id, label, proto}]
var tool_index := 0
var _tool_buttons: Array[Button] = []

# entrees tactiles
var _touches := {}
var _press_pos := Vector2.ZERO
var _moved := false
var _pinched := false
var _pinch_dist := 0.0

# interface
var ui_root: Control
var top_bar: PanelContainer
var status_label: Label
var toolbar: PanelContainer
var tool_grid: GridContainer
var tool_scroll: ScrollContainer
var action_bar: HBoxContainer
var info_panel: PanelContainer
var info_title: Label
var info_body: Label
var info_recipes: VBoxContainer
var menu_panel: PanelContainer
var message: Label
var _message_timer := 0.0
var _info_tile := Vector2i.ZERO
var _info_open := false
var _gear_item := -1


func _ready() -> void:
	if not ClassDB.class_exists("ExodeSim"):
		_show_missing_library()
		return
	sim = ClassDB.instantiate("ExodeSim")
	if "--selftest" in OS.get_cmdline_user_args():
		_selftest()
		return
	view = WorldView.new()
	add_child(view)
	if not _load_game(AUTOSAVE):
		_new_game(true)
	view.setup(sim)
	_gear_item = Array(sim.item_keys()).find("engrenage")
	_build_tools()
	_build_ui()
	get_viewport().size_changed.connect(_layout)
	_layout()
	_select_tool(0)
	for a in OS.get_cmdline_user_args():
		if a.begins_with("--screenshot="):
			_screenshot(a.substr(13))


# ---------------------------------------------------------------- partie

func _new_game(with_demo: bool) -> void:
	var seed := 1 if with_demo else randi() % 1000000
	sim.new_game(seed, false)
	if with_demo:
		sim.build_demo()
	if view:
		view.cam_x = -3.0
		view.cam_y = 0.5
		view.zoom = 40.0
		view.setup(sim)


func _save_game(path: String) -> bool:
	var bytes: PackedByteArray = sim.save_bytes()
	var tmp := AUTOSAVE_TMP
	var f := FileAccess.open(tmp, FileAccess.WRITE)
	if f == null:
		return false
	f.store_buffer(bytes)
	f.flush()
	f.close()
	# remplacement atomique : la derniere bonne sauvegarde n'est jamais corrompue
	var abs_tmp := ProjectSettings.globalize_path(tmp)
	var abs_path := ProjectSettings.globalize_path(path)
	if DirAccess.rename_absolute(abs_tmp, abs_path) != OK:
		DirAccess.remove_absolute(abs_path)
		return DirAccess.rename_absolute(abs_tmp, abs_path) == OK
	return true


func _load_game(path: String) -> bool:
	if not FileAccess.file_exists(path):
		return false
	var bytes := FileAccess.get_file_as_bytes(path)
	if bytes.is_empty():
		return false
	return sim.load_bytes(bytes)


func _notification(what: int) -> void:
	if sim == null or view == null:
		return
	if what == NOTIFICATION_APPLICATION_PAUSED or what == NOTIFICATION_WM_CLOSE_REQUEST or what == NOTIFICATION_APPLICATION_FOCUS_OUT:
		_save_game(AUTOSAVE)


# ---------------------------------------------------------------- boucle

func _process(delta: float) -> void:
	if sim == null or view == null:
		return
	if not paused:
		_acc += delta * float(sim.ticks_per_second())
		var n := mini(int(_acc), MAX_TICKS_PER_FRAME)
		_acc -= n
		if _acc > MAX_TICKS_PER_FRAME:
			_acc = 0.0 # materiel sature : la simulation ralentit, les regles ne changent pas
		if n > 0:
			sim.step(n)
	_autosave_timer += delta
	if _autosave_timer > AUTOSAVE_PERIOD:
		_autosave_timer = 0.0
		_save_game(AUTOSAVE)
	_ui_timer += delta
	if _ui_timer > 0.25:
		_ui_timer = 0.0
		_update_status()
		if _info_open:
			_refresh_info()
	if _message_timer > 0.0:
		_message_timer -= delta
		if _message_timer <= 0.0:
			message.visible = false


func _update_status() -> void:
	var p: PackedInt64Array = sim.power_summary()
	var sat: int = p[2] / 10 if p[3] > 0 else 0
	var gears: int = sim.produced(_gear_item) if _gear_item >= 0 else 0
	status_label.text = "%s  Tick %d  |  %.2f ms/tick  |  Energie %d/%d kW (%d%%)  |  Engrenages %d" % [
		"PAUSE " if paused else "", sim.tick(), sim.last_step_ms(), p[1] / 1000, p[0] / 1000, sat, gears]


# ---------------------------------------------------------------- outils

func _build_tools() -> void:
	tools.clear()
	tools.append({"id": "inspect", "label": "Inspecter", "proto": -1})
	var keys: PackedStringArray = sim.proto_keys()
	var names: PackedStringArray = sim.proto_names()
	for i in keys.size():
		var label := names[i].split(" (")[0]
		tools.append({"id": "build", "label": label, "proto": i})
	tools.append({"id": "remove", "label": "Demolir", "proto": -1})
	tools.append({"id": "clear", "label": "Defricher", "proto": -1})
	tools.append({"id": "fill", "label": "Remblayer", "proto": -1})


func _select_tool(i: int) -> void:
	tool_index = i
	for k in _tool_buttons.size():
		_tool_buttons[k].button_pressed = (k == i)
	view.ghost_proto = -1
	view.marker_on = false
	view.selected_id = -1
	action_bar.visible = false
	_close_info()


func _current_tool() -> Dictionary:
	return tools[tool_index]


func _on_tap(screen_pos: Vector2) -> void:
	var t := view.screen_to_tile(screen_pos)
	var tool := _current_tool()
	match tool.id:
		"inspect":
			_open_info(t)
		"build":
			_place_ghost(tool.proto, t)
		"remove", "clear", "fill":
			view.marker = t
			view.marker_on = true
			view.marker_color = Color(1, 0.3, 0.2) if tool.id == "remove" else Color(0.9, 0.9, 0.3)
			action_bar.visible = true


func _place_ghost(proto: int, t: Vector2i) -> void:
	var sizes: PackedInt32Array = sim.proto_sizes()
	var w := sizes[proto * 2]
	var h := sizes[proto * 2 + 1]
	view.ghost_proto = proto
	view.ghost_size = Vector2i(w, h)
	view.ghost_x = t.x - (w - 1) / 2
	view.ghost_y = t.y - (h - 1) / 2
	view.ghost_radius = sim.proto_supply_radius(proto)
	_update_ghost()
	action_bar.visible = true


func _update_ghost() -> void:
	view.ghost_ok = sim.can_build(view.ghost_proto, view.ghost_x, view.ghost_y, view.ghost_dir)


func _on_rotate() -> void:
	if view.ghost_proto >= 0:
		view.ghost_dir = (view.ghost_dir + 1) % 4
		_update_ghost()
	elif _info_open:
		sim.rotate_at(_info_tile.x, _info_tile.y)


func _on_cancel() -> void:
	view.ghost_proto = -1
	view.marker_on = false
	action_bar.visible = false


func _on_confirm() -> void:
	var tool := _current_tool()
	match tool.id:
		"build":
			var err: String = sim.build_error(view.ghost_proto, view.ghost_x, view.ghost_y, view.ghost_dir)
			if err != "":
				_show_message(err)
				return
			sim.build(view.ghost_proto, view.ghost_x, view.ghost_y, view.ghost_dir)
			# pose en ligne : le fantome avance d'une case pour les convoyeurs et les poteaux
			var kind: String = view.proto_kinds[view.ghost_proto]
			if kind == "belt":
				var v := view._dir_vec(view.ghost_dir)
				view.ghost_x += int(v.x)
				view.ghost_y += int(v.y)
			_update_ghost()
		"remove":
			if not sim.remove_at(view.marker.x, view.marker.y):
				_show_message("Rien a demolir ici")
		"clear":
			if sim.clear_feature(view.marker.x, view.marker.y):
				view.invalidate_terrain()
			else:
				_show_message("Ni arbre ni rocher ici")
		"fill":
			if sim.fill_water(view.marker.x, view.marker.y):
				view.invalidate_terrain()
			else:
				_show_message("Pas d'eau ici")


# ---------------------------------------------------------------- panneau d'information

func _open_info(t: Vector2i) -> void:
	var info: Dictionary = sim.entity_info(t.x, t.y)
	if info.is_empty():
		_close_info()
		return
	_info_tile = t
	_info_open = true
	view.selected_id = info.id
	info_panel.visible = true
	action_bar.visible = true
	_refresh_info()
	# recettes au choix
	for c in info_recipes.get_children():
		c.queue_free()
	var cat: String = info.category
	var proto: int = info.proto
	if cat != "":
		var auto: bool = view.proto_names[proto].begins_with("Four") or view.proto_names[proto].begins_with("Recycl")
		if auto:
			var l := Label.new()
			l.text = "Recette choisie automatiquement selon l'objet recu."
			l.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
			info_recipes.add_child(l)
		else:
			var cats: PackedStringArray = sim.recipe_categories()
			for r in cats.size():
				if cats[r] == cat:
					var b := Button.new()
					b.text = sim.recipe_label(r)
					b.custom_minimum_size = Vector2(0, 44)
					b.pressed.connect(func(): sim.set_recipe_at(_info_tile.x, _info_tile.y, r); _refresh_info())
					info_recipes.add_child(b)


func _refresh_info() -> void:
	var info: Dictionary = sim.entity_info(_info_tile.x, _info_tile.y)
	if info.is_empty():
		_close_info()
		return
	info_title.text = info.name
	var lines := ["Etat : %s" % STATUS_TEXT[info.status]]
	if info.needs_power:
		lines.append("Energie : %d %%" % (info.power / 10))
	if info.recipe >= 0:
		lines.append("Recette : %s" % sim.recipe_label(info.recipe))
	if info.contents != "":
		lines.append(info.contents)
	info_body.text = "\n".join(lines)


func _close_info() -> void:
	_info_open = false
	if info_panel:
		info_panel.visible = false
	if view:
		view.selected_id = -1


# ---------------------------------------------------------------- entrees

func _unhandled_input(event: InputEvent) -> void:
	if view == null:
		return
	if event is InputEventScreenTouch:
		if event.pressed:
			_touches[event.index] = event.position
			if _touches.size() == 1:
				_press_pos = event.position
				_moved = false
				_pinched = false
			elif _touches.size() == 2:
				_pinched = true
				_pinch_dist = _touch_distance()
		else:
			_touches.erase(event.index)
			if _touches.is_empty() and not _moved and not _pinched:
				_on_tap(event.position)
	elif event is InputEventScreenDrag:
		_touches[event.index] = event.position
		if _touches.size() == 1:
			if event.position.distance_to(_press_pos) > TAP_SLOP:
				_moved = true
			if _moved:
				view.cam_x -= event.relative.x / view.zoom
				view.cam_y -= event.relative.y / view.zoom
		elif _touches.size() == 2:
			var d := _touch_distance()
			if _pinch_dist > 0.0:
				_zoom_at(_touch_center(), d / _pinch_dist)
			_pinch_dist = d
	elif event is InputEventMouseButton and event.pressed:
		if event.button_index == MOUSE_BUTTON_WHEEL_UP:
			_zoom_at(event.position, 1.15)
		elif event.button_index == MOUSE_BUTTON_WHEEL_DOWN:
			_zoom_at(event.position, 1.0 / 1.15)
	elif event is InputEventMagnifyGesture:
		_zoom_at(event.position, event.factor)


func _touch_distance() -> float:
	var p: Array = _touches.values()
	return (p[0] as Vector2).distance_to(p[1]) if p.size() >= 2 else 0.0


func _touch_center() -> Vector2:
	var p: Array = _touches.values()
	return ((p[0] as Vector2) + (p[1] as Vector2)) * 0.5 if p.size() >= 2 else Vector2.ZERO


func _zoom_at(screen_pos: Vector2, factor: float) -> void:
	var s := view.view_size()
	var before_x := view.cam_x + (screen_pos.x - s.x * 0.5) / view.zoom
	var before_y := view.cam_y + (screen_pos.y - s.y * 0.5) / view.zoom
	view.zoom = clampf(view.zoom * factor, ZOOM_MIN, ZOOM_MAX)
	view.cam_x = before_x - (screen_pos.x - s.x * 0.5) / view.zoom
	view.cam_y = before_y - (screen_pos.y - s.y * 0.5) / view.zoom


# ---------------------------------------------------------------- interface

func _button(text: String, cb: Callable, min_size := Vector2(64, 48)) -> Button:
	var b := Button.new()
	b.text = text
	b.custom_minimum_size = min_size
	b.focus_mode = Control.FOCUS_NONE
	b.pressed.connect(cb)
	return b


func _build_ui() -> void:
	var layer := CanvasLayer.new()
	add_child(layer)
	ui_root = Control.new()
	ui_root.set_anchors_preset(Control.PRESET_FULL_RECT)
	ui_root.mouse_filter = Control.MOUSE_FILTER_IGNORE
	layer.add_child(ui_root)

	# barre du haut
	top_bar = PanelContainer.new()
	ui_root.add_child(top_bar)
	var top := HBoxContainer.new()
	top_bar.add_child(top)
	status_label = Label.new()
	status_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	status_label.clip_text = true
	status_label.add_theme_font_size_override("font_size", 13)
	top.add_child(status_label)
	top.add_child(_button("Pause", func(): paused = not paused; _update_status(), Vector2(64, 40)))
	top.add_child(_button("Menu", func(): menu_panel.visible = not menu_panel.visible, Vector2(64, 40)))

	# barre d'outils
	toolbar = PanelContainer.new()
	ui_root.add_child(toolbar)
	tool_scroll = ScrollContainer.new()
	toolbar.add_child(tool_scroll)
	tool_grid = GridContainer.new()
	tool_scroll.add_child(tool_grid)
	var group := ButtonGroup.new()
	_tool_buttons.clear()
	for i in tools.size():
		var b := _button(tools[i].label, _select_tool.bind(i), Vector2(84, 48))
		b.toggle_mode = true
		b.button_group = group
		b.add_theme_font_size_override("font_size", 13)
		tool_grid.add_child(b)
		_tool_buttons.append(b)

	# actions de construction : tourner, annuler, valider (confirmation explicite, v0.1 2.3)
	action_bar = HBoxContainer.new()
	ui_root.add_child(action_bar)
	action_bar.add_child(_button("Tourner", _on_rotate))
	action_bar.add_child(_button("Annuler", _on_cancel))
	var ok := _button("Valider", _on_confirm, Vector2(84, 48))
	ok.add_theme_color_override("font_color", Color(0.5, 1.0, 0.5))
	action_bar.add_child(ok)
	action_bar.visible = false

	# panneau d'information
	info_panel = PanelContainer.new()
	ui_root.add_child(info_panel)
	var iv := VBoxContainer.new()
	info_panel.add_child(iv)
	var head := HBoxContainer.new()
	iv.add_child(head)
	info_title = Label.new()
	info_title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	head.add_child(info_title)
	head.add_child(_button("X", _close_info, Vector2(44, 40)))
	info_body = Label.new()
	info_body.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	info_body.add_theme_font_size_override("font_size", 13)
	iv.add_child(info_body)
	info_recipes = VBoxContainer.new()
	iv.add_child(info_recipes)
	info_panel.visible = false

	# menu
	menu_panel = PanelContainer.new()
	ui_root.add_child(menu_panel)
	var mv := VBoxContainer.new()
	menu_panel.add_child(mv)
	mv.add_child(_button("Sauvegarder", func(): _show_message("Partie sauvegardee" if _save_game(AUTOSAVE) else "Echec de la sauvegarde"); menu_panel.visible = false, Vector2(220, 48)))
	mv.add_child(_button("Recharger la sauvegarde", func(): _reload(); menu_panel.visible = false, Vector2(220, 48)))
	mv.add_child(_button("Nouvelle partie (scene de demo)", func(): _new_game(true); menu_panel.visible = false, Vector2(220, 48)))
	mv.add_child(_button("Nouvelle partie (monde vide)", func(): _new_game(false); menu_panel.visible = false, Vector2(220, 48)))
	mv.add_child(_button("Liberer la memoire", func(): _show_message("%d zones liberees" % sim.drop_unmodified_chunks()); view.invalidate_terrain(); menu_panel.visible = false, Vector2(220, 48)))
	mv.add_child(_button("Fermer", func(): menu_panel.visible = false, Vector2(220, 48)))
	menu_panel.visible = false

	message = Label.new()
	message.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	message.add_theme_color_override("font_color", Color(1, 0.85, 0.4))
	message.add_theme_color_override("font_outline_color", Color.BLACK)
	message.add_theme_constant_override("outline_size", 4)
	ui_root.add_child(message)
	message.visible = false


func _reload() -> void:
	if _load_game(AUTOSAVE):
		view.setup(sim)
		_show_message("Sauvegarde rechargee")
	else:
		_show_message("Aucune sauvegarde valide")


## Portrait : outils en bas. Paysage : outils a gauche (v0.1 2.1).
func _layout() -> void:
	if ui_root == null:
		return
	var s := get_viewport().get_visible_rect().size
	var portrait := s.y > s.x
	var top_h := 44.0
	top_bar.position = Vector2.ZERO
	top_bar.size = Vector2(s.x, top_h)
	if portrait:
		var tb_h := 64.0
		toolbar.position = Vector2(0, s.y - tb_h)
		toolbar.size = Vector2(s.x, tb_h)
		tool_grid.columns = tools.size()
		tool_scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_AUTO
		tool_scroll.vertical_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
		action_bar.position = Vector2(s.x - 240, s.y - tb_h - 56)
		info_panel.position = Vector2(8, top_h + 8)
		info_panel.size = Vector2(s.x - 16, 0)
	else:
		var tb_w := 190.0
		toolbar.position = Vector2(0, top_h)
		toolbar.size = Vector2(tb_w, s.y - top_h)
		tool_grid.columns = 2
		tool_scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
		tool_scroll.vertical_scroll_mode = ScrollContainer.SCROLL_MODE_AUTO
		action_bar.position = Vector2(s.x - 240, s.y - 56)
		info_panel.position = Vector2(s.x - 330, top_h + 8)
		info_panel.size = Vector2(320, 0)
	action_bar.size = Vector2(232, 48)
	menu_panel.position = Vector2(s.x - 240, top_h + 4)
	message.position = Vector2(0, s.y * 0.6)
	message.size = Vector2(s.x, 30)


func _show_message(text: String) -> void:
	message.text = text
	message.visible = true
	_message_timer = 2.5


func _show_missing_library() -> void:
	var l := Label.new()
	l.text = "Bibliotheque native EXODE absente.\nCompiler le noyau (voir README) ou recuperer l'APK construit par GitHub Actions."
	l.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	l.set_anchors_preset(Control.PRESET_FULL_RECT)
	l.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	l.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	var layer := CanvasLayer.new()
	add_child(layer)
	layer.add_child(l)
	push_error("ExodeSim introuvable : bibliotheque native non chargee")


## Capture d'ecran de controle (outil de developpement) : quelques secondes de jeu puis image.
func _screenshot(path: String) -> void:
	_new_game(true)
	sim.step(900)
	view.cam_x = -5.0
	view.cam_y = 0.5
	view.zoom = 52.0
	_open_info(Vector2i(-2, -4))
	_select_tool_keep_info()
	if "--lac" in OS.get_cmdline_user_args():
		# outil de controle : centre la vue sur le premier lac trouve
		_close_info()
		for rad in range(30, 400, 6):
			var found := false
			for a in 32:
				var tx := int(rad * cos(a * TAU / 32.0))
				var ty := int(rad * sin(a * TAU / 32.0))
				if view._ground_at(tx, ty) == 3:
					view.cam_x = tx
					view.cam_y = ty
					found = true
					break
			if found:
				break
		view.zoom = 40.0
	for i in 30:
		await get_tree().process_frame
	var img := get_viewport().get_texture().get_image()
	img.save_png(path)
	print("CAPTURE ", path)
	get_tree().quit()


func _select_tool_keep_info() -> void:
	_tool_buttons[0].button_pressed = true


## Verification sans interface (CI) : charge l'extension, fait tourner la demo, sauvegarde/recharge.
func _selftest() -> void:
	sim.new_game(42, false)
	var ok: bool = sim.build_demo()
	sim.step(3000)
	var h1: String = sim.state_hash()
	var bytes: PackedByteArray = sim.save_bytes()
	var other = ClassDB.instantiate("ExodeSim")
	ok = ok and other.load_bytes(bytes)
	ok = ok and other.state_hash() == h1
	ok = ok and sim.mass_balance_ok()
	var gear := Array(sim.item_keys()).find("engrenage")
	var gears: int = sim.produced(gear)
	ok = ok and gears > 0
	print("SELFTEST %s tick=%d engrenages=%d empreinte=%s entites=%d" % ["OK" if ok else "ECHEC", sim.tick(), gears, h1, sim.entity_count()])
	get_tree().quit(0 if ok else 1)
