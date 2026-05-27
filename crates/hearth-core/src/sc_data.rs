//! Single adapter point between Hearth and sc-holotable types.
//!
//! Concentrating contact with sc-holotable here keeps the blast radius
//! of upstream churn to a single module. When sc-holotable bumps a
//! breaking type, only this file (plus its tests) need to change.
