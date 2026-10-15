// Private test sessions only: enables org.gnome.Shell.Eval for acceptance tests.
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

export default class EvalFixture extends Extension {
    enable() {
        global.context.unsafe_mode = true;
    }

    disable() {
        global.context.unsafe_mode = false;
    }
}
