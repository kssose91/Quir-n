//! # Design
//!
//! Sistema de diseño de la interfaz: escala tipográfica, rejilla de espaciado y
//! factor de escala.
//!
//! Antes de existir este módulo, el render usaba once tamaños de letra distintos
//! —incluido un `9.8`— y 277 desplazamientos literales, de los que apenas el 41 %
//! caía en una rejilla. Ni un solo control permitía agrandar la interfaz: el
//! ajuste de fuente afectaba únicamente al editor de código.
//!
//! Las etiquetas se dibujaban entre 9 y 10,5 píxeles, cuando VS Code usa 13 y
//! GNOME cerca de 15. De ahí que no se leyeran.

/// Escala tipográfica. Cinco pasos, y ninguno más.
///
/// Los valores son tamaños lógicos: el factor de escala los multiplica al
/// dibujar.
/// Los cinco pasos están muestreados del rediseño «Modernist» (`diseño/Llore
/// Rediseño.dc.html`, 4-sep-2026), no elegidos a ojo: se leyeron del DOM
/// renderizado de la maqueta. La escala anterior venía de la maqueta vieja y
/// se quedaba corta —el cuerpo de la interfaz iba a 12 px cuando VS Code usa
/// 13 y la maqueta pide 13—, que es de donde salía buena parte de la sensación
/// de interfaz apretada.
pub mod type_scale {
    /// Etiquetas secundarias: meta de un mensaje, rutas, contadores.
    pub const XS: f32 = 11.5;
    /// Cuerpo de la interfaz: menús, pestañas, explorador, barra de estado.
    pub const SM: f32 = 13.0;
    /// Texto principal: mensajes del chat y su entrada.
    pub const MD: f32 = 14.5;
    /// Títulos de panel y encabezados.
    pub const LG: f32 = 20.0;
    /// Título de la pantalla de bienvenida.
    pub const XL: f32 = 42.0;
}

/// Rejilla de espaciado. Todo múltiplo de cuatro.
pub mod space {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
}

/// Radio de las esquinas. Tres pasos, muestreados de la maqueta.
///
/// Antes de existir este módulo el render tenía dieciocho radios literales
/// entre 3 y 12 px, elegidos uno a uno. La maqueta es más decidida: 10 en los
/// botones y campos, 16 en las tarjetas. Esa timidez era buena parte de lo que
/// hacía que la interfaz se viera tosca.
pub mod radius {
    /// Fichas, píldoras y controles pequeños.
    pub const SM: f32 = 6.0;
    /// Botones y campos de entrada.
    pub const MD: f32 = 10.0;
    /// Tarjetas y paneles.
    pub const LG: f32 = 16.0;
}

/// Escala mínima de la interfaz.
pub const UI_SCALE_MIN: f32 = 0.75;
/// Escala máxima de la interfaz.
pub const UI_SCALE_MAX: f32 = 2.5;
/// Incremento de cada paso de escala.
pub const UI_SCALE_STEP: f32 = 0.1;

/// Factor de escala efectivo de la interfaz.
///
/// Combina la preferencia del usuario con el factor que anuncia el monitor. El
/// segundo no se elegía nunca: el rasterizado usaba `1.0` fijo, de modo que en
/// una pantalla de alta densidad la interfaz salía a la mitad de tamaño.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    user: f32,
    device: f32,
}

impl Scale {
    pub fn new(user: f32, device: f32) -> Self {
        Self {
            user: user.clamp(UI_SCALE_MIN, UI_SCALE_MAX),
            device: if device.is_finite() && device > 0.0 {
                device
            } else {
                1.0
            },
        }
    }

    /// Factor total por el que se multiplica todo lo que se dibuja.
    pub fn factor(&self) -> f32 {
        self.user * self.device
    }

    /// Preferencia del usuario, sin el factor del monitor.
    pub fn user(&self) -> f32 {
        self.user
    }

    pub fn set_user(&mut self, user: f32) {
        self.user = user.clamp(UI_SCALE_MIN, UI_SCALE_MAX);
    }

    pub fn set_device(&mut self, device: f32) {
        if device.is_finite() && device > 0.0 {
            self.device = device;
        }
    }

    /// Sube un paso, sin pasar del máximo.
    pub fn increase(&mut self) {
        self.set_user(self.user + UI_SCALE_STEP);
    }

    /// Baja un paso, sin bajar del mínimo.
    pub fn decrease(&mut self) {
        self.set_user(self.user - UI_SCALE_STEP);
    }

    /// Vuelve a la escala natural del monitor.
    pub fn reset(&mut self) {
        self.user = 1.0;
    }

    /// Convierte un tamaño físico de ventana en el tamaño lógico que ve el
    /// layout.
    pub fn to_logical(&self, physical: u32) -> u32 {
        let factor = self.factor();
        if factor <= 0.0 {
            return physical;
        }
        ((physical as f32) / factor).round().max(1.0) as u32
    }
}

impl Default for Scale {
    fn default() -> Self {
        Self::new(1.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_escala_combina_usuario_y_monitor() {
        let scale = Scale::new(1.25, 2.0);
        assert_eq!(scale.factor(), 2.5);
        assert_eq!(scale.user(), 1.25);
    }

    #[test]
    fn la_preferencia_del_usuario_se_acota() {
        let mut scale = Scale::default();

        for _ in 0..100 {
            scale.increase();
        }
        assert_eq!(scale.user(), UI_SCALE_MAX);

        for _ in 0..100 {
            scale.decrease();
        }
        assert_eq!(scale.user(), UI_SCALE_MIN);
    }

    #[test]
    fn un_factor_de_monitor_invalido_no_rompe_la_escala() {
        let scale = Scale::new(1.0, 0.0);
        assert_eq!(scale.factor(), 1.0);

        let scale = Scale::new(1.0, f32::NAN);
        assert_eq!(scale.factor(), 1.0);
    }

    #[test]
    fn el_tamano_logico_encoge_al_crecer_la_escala() {
        let scale = Scale::new(2.0, 1.0);
        assert_eq!(scale.to_logical(1280), 640);

        let natural = Scale::default();
        assert_eq!(natural.to_logical(1280), 1280);
    }

    #[test]
    fn reset_devuelve_la_escala_natural_del_monitor() {
        let mut scale = Scale::new(1.0, 2.0);
        scale.increase();
        assert!(scale.user() > 1.0);

        scale.reset();
        assert_eq!(scale.user(), 1.0);
        assert_eq!(scale.factor(), 2.0, "el monitor sigue mandando");
    }

    #[test]
    fn la_escala_tipografica_es_creciente() {
        use type_scale::*;
        assert!(XS < SM && SM < MD && MD < LG && LG < XL);
    }

    #[test]
    fn la_rejilla_es_multiplo_de_cuatro() {
        use space::*;
        for valor in [XS, SM, MD, LG, XL, XXL] {
            assert_eq!(valor % 4.0, 0.0, "{valor} rompe la rejilla");
        }
    }
}
