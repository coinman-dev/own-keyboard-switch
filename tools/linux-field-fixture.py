#!/usr/bin/env python3
"""Two fields for isolated Linux acceptance. Contents are test strings only."""
import json
from pathlib import Path
import sys
import gi
gi.require_version("Gtk","3.0")
from gi.repository import Gtk, GLib

state, command = map(Path,sys.argv[1:3])
window=Gtk.Window(title="OKBS two-field acceptance")
box=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=4)
first,second=Gtk.Entry(),Gtk.Entry()
box.add(first);box.add(second);window.add(box)
menubar=Gtk.MenuBar()
box.pack_start(menubar,False,False,0)
box.reorder_child(menubar,0)
menu_open=False

def menu_state(active):
    global menu_open
    menu_open=active
    snapshot()

def set_menu(language):
    for item in menubar.get_children(): item.destroy()
    labels={"en":["_File","_Edit"],"ru":["_Файл","_Правка"],"none":["File","Edit"]}[language]
    for label in labels:
        item=Gtk.MenuItem.new_with_mnemonic(label) if language!="none" else Gtk.MenuItem.new_with_label(label)
        submenu=Gtk.Menu()
        submenu.add(Gtk.MenuItem.new_with_label("Fixture action"))
        item.set_submenu(submenu)
        item.connect("select",lambda *_: menu_state(True))
        item.connect("deselect",lambda *_: menu_state(False))
        menubar.add(item)
    menubar.show_all()
    first.grab_focus()
def snapshot(*_):
    temporary=state.with_suffix(".new")
    temporary.write_text(json.dumps({"first":first.get_text(),"second":second.get_text(),"menu_open":menu_open},ensure_ascii=False))
    temporary.replace(state)
def poll():
    if command.exists():
        action=command.read_text().strip()
        command.unlink()
        if action=="second": second.grab_focus()
        if action=="first": first.grab_focus()
        if action=="password": second.set_visibility(False);second.grab_focus()
        if action.startswith("menu-"): set_menu(action.removeprefix("menu-"))
    return True
first.connect("changed",snapshot);second.connect("changed",snapshot)
window.connect("destroy",Gtk.main_quit)
window.show_all();first.grab_focus();snapshot()
GLib.timeout_add(10,poll)
Gtk.main()
