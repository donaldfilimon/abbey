#![cfg(feature = "gtk-runtime")]
use gtk::{glib, prelude::*, subclass::prelude::*};

mod imp {
    use super::*;
    #[derive(Default, gtk::CompositeTemplate)]
    #[template(
        string = "<interface><template class='AbbeyCompatTemplate' parent='GtkBox'><child><object class='GtkLabel' id='label'><property name='label'>Abbey GTK witness</property></object></child></template></interface>"
    )]
    pub struct Template {
        #[template_child]
        pub label: gtk::TemplateChild<gtk::Label>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Template {
        const NAME: &'static str = "AbbeyCompatTemplate";
        type Type = super::Template;
        type ParentType = gtk::Box;
        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }
        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }
    impl ObjectImpl for Template {}
    impl WidgetImpl for Template {}
    impl ContainerImpl for Template {}
    impl BoxImpl for Template {}
}
glib::wrapper! {
    pub struct Template(ObjectSubclass<imp::Template>)
        @extends gtk::Widget, gtk::Container, gtk::Box,
        @implements gtk::Buildable, gtk::Orientable;
}

#[test]
fn native_template_binds_declared_child() {
    gtk::init().expect("GTK3 display must be supplied by the native runner");
    let widget: Template = glib::Object::new();
    assert_eq!(widget.imp().label.label(), "Abbey GTK witness");
}
