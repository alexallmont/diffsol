use std::ffi::c_void;
use std::fmt::{Debug, Display, Formatter};
use std::ops::{Add, AddAssign, Div, DivAssign, Index, IndexMut, Mul, MulAssign, Sub, SubAssign};
use std::ptr::addr_of_mut;
use std::{fmt, ptr};

use sundials_sys::{
    realtype, N_VAbs, N_VAddConst, N_VClone, N_VConst, N_VDestroy, N_VDiv, N_VGetArrayPointer,
    N_VGetLength_Serial, N_VLinearSum, N_VNew_Serial, N_VProd, N_VScale, N_VWL2Norm_Serial,
    N_Vector, SUNContext, SUNContext_Create,
};

use crate::{scale, IndexType, Scale};

use super::{Vector, VectorCommon, VectorIndex, VectorView, VectorViewMut};

static mut SUNCONTEXT: SUNContext = std::ptr::null_mut();

pub fn get_suncontext() -> &'static SUNContext {
    let sun_comm_null: *mut c_void = ptr::null_mut::<c_void>();
    unsafe {
        if SUNCONTEXT.is_null() {
            SUNContext_Create(sun_comm_null, addr_of_mut!(SUNCONTEXT) as *mut SUNContext);
        }
        &SUNCONTEXT
    }
}

#[derive(Debug)]
pub struct SundialsVector {
    nv: N_Vector,
    // hack to allow to take a N_Vector and pass to an Op call (which needs a &V).
    // TODO: find a better way to handle this, perhaps Op calls should take a View?
    owned: bool,
}

impl SundialsVector {
    pub fn new_serial(len: usize) -> Self {
        let ctx = get_suncontext();
        let nv = unsafe { N_VNew_Serial(len as i64, *ctx) };
        SundialsVector { nv, owned: true }
    }
    /// Create a new SundialsVector with the same length and type as the given vector.
    /// data is not copied.
    pub fn new_clone(v: &SundialsVector) -> Self {
        let nv = unsafe { N_VClone(v.nv) };
        SundialsVector { nv, owned: true }
    }
    pub fn new_not_owned(v: N_Vector) -> Self {
        if v.is_null() {
            panic!("N_Vector is null");
        }
        SundialsVector {
            nv: v,
            owned: false,
        }
    }
    pub fn sundials_vector(&self) -> N_Vector {
        self.nv
    }
}

impl Drop for SundialsVector {
    fn drop(&mut self) {
        if self.owned {
            unsafe { N_VDestroy(self.nv) };
        }
    }
}

#[derive(Debug)]
pub struct SundialsVectorViewMut<'a>(&'a mut SundialsVector);

impl<'a> SundialsVectorViewMut<'a> {
    fn sundials_vector(&self) -> N_Vector {
        self.0.sundials_vector()
    }
    fn len(&self) -> IndexType {
        unsafe { N_VGetLength_Serial(self.sundials_vector()) as IndexType }
    }
}

#[derive(Debug)]
pub struct SundialsVectorView<'a>(&'a SundialsVector);

impl<'a> SundialsVectorView<'a> {
    fn sundials_vector(&self) -> N_Vector {
        self.0.sundials_vector()
    }
    fn len(&self) -> IndexType {
        unsafe { N_VGetLength_Serial(self.sundials_vector()) as IndexType }
    }
}

#[derive(Debug)]
pub struct SundialsIndexVector(Vec<IndexType>);

impl Display for SundialsIndexVector {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        for i in 0..self.0.len() {
            write!(f, "{} ", self.0[i])?;
        }
        writeln!(f)?;
        Ok(())
    }
}

impl Display for SundialsVector {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        for i in 0..self.len() {
            write!(f, "{} ", self[i])?;
        }
        writeln!(f)?;
        Ok(())
    }
}

impl Display for SundialsVectorViewMut<'_> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        for i in 0..self.len() {
            write!(f, "{} ", self[i])?;
        }
        writeln!(f)?;
        Ok(())
    }
}

impl Display for SundialsVectorView<'_> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        for i in 0..self.len() {
            write!(f, "{} ", self[i])?;
        }
        writeln!(f)?;
        Ok(())
    }
}

impl VectorCommon for SundialsVector {
    type T = realtype;
}

impl<'a> VectorCommon for SundialsVectorView<'a> {
    type T = realtype;
}

impl<'a> VectorCommon for SundialsVectorViewMut<'a> {
    type T = realtype;
}

macro_rules! impl_helper {
    ($trait:path, SundialsVector, $inner:tt) => {
        impl $trait for SundialsVector $inner
    };
    ($trait:path, SundialsVectorView, $inner:tt) => {
        impl<'a> $trait for SundialsVectorView<'a> $inner
    };
    ($trait:path, SundialsVectorViewMut, $inner:tt) => {
        impl<'a> $trait for SundialsVectorViewMut<'a> $inner
    };
}

// Clone
impl Clone for SundialsVector {
    fn clone(&self) -> Self {
        let mut z = SundialsVector::new_serial(self.len());
        z.copy_from(self);
        z
    }
}

// Index
macro_rules! impl_index {
    ($type:tt) => {
        impl_helper!(Index<IndexType>, $type, {
            type Output = <Self as VectorCommon>::T;
            fn index(&self, index: usize) -> &Self::Output {
                unsafe { &*(N_VGetArrayPointer(self.sundials_vector()).add(index)) }
            }
        });
    };
}

impl_index!(SundialsVector);
impl_index!(SundialsVectorView);
impl_index!(SundialsVectorViewMut);

impl Index<IndexType> for SundialsIndexVector {
    type Output = IndexType;
    fn index(&self, index: IndexType) -> &Self::Output {
        &self.0[index]
    }
}

// IndexMut
macro_rules! impl_index_mut {
    ($type:tt) => {
        impl_helper!(IndexMut<IndexType>, $type, {
            fn index_mut(&mut self, index: usize) -> &mut Self::Output {
                unsafe { &mut *(N_VGetArrayPointer(self.sundials_vector()).add(index)) }
            }
        });
    };
}

impl_index_mut!(SundialsVector);
impl_index_mut!(SundialsVectorViewMut);

// div by realtype -> owned
macro_rules! impl_div {
    ($type:tt) => {
        impl_helper!(Div<Scale<realtype>>, $type, {
            type Output = SundialsVector;
            fn div(self, rhs: Scale<realtype>) -> Self::Output {
                let z = SundialsVector::new_serial(self.len());
                unsafe {
                    N_VScale(
                        1. / rhs.value(),
                        self.sundials_vector(),
                        z.sundials_vector(),
                    )
                }
                z
            }
        });
    };
}

impl_div!(SundialsVector);
impl_div!(SundialsVectorView);
impl_div!(SundialsVectorViewMut);

// mul by realtype -> owned
macro_rules! impl_mul {
    ($type:tt) => {
        impl_helper!(Mul<Scale<realtype>>, $type, {
            type Output = SundialsVector;
            fn mul(self, rhs: Scale<realtype>) -> Self::Output {
                let z = SundialsVector::new_serial(self.len());
                unsafe { N_VScale(rhs.value(), self.sundials_vector(), z.sundials_vector()) }
                z
            }
        });
    };
}

impl_mul!(SundialsVector);
impl_mul!(SundialsVectorView);
impl_mul!(SundialsVectorViewMut);

// div assign with realtype
macro_rules! impl_div_assign {
    ($type:tt) => {
        impl_helper!(DivAssign<Scale<realtype>>, $type, {
            fn div_assign(&mut self, rhs: Scale<realtype>) {
                unsafe {
                    N_VScale(
                        1. / rhs.value(),
                        self.sundials_vector(),
                        self.sundials_vector(),
                    )
                }
            }
        });
    };
}

impl_div_assign!(SundialsVectorViewMut);
impl_div_assign!(SundialsVector);
impl_div_assign!(SundialsVectorView);

// mul assign with realtype
macro_rules! impl_mul_assign {
    ($type:tt) => {
        impl_helper!(MulAssign<Scale<realtype>>, $type, {
            fn mul_assign(&mut self, rhs: Scale<realtype>) {
                unsafe { N_VScale(rhs.value(), self.sundials_vector(), self.sundials_vector()) }
            }
        });
    };
}

impl_mul_assign!(SundialsVectorViewMut);
impl_mul_assign!(SundialsVector);
impl_mul_assign!(SundialsVectorView);

// op assign with owned and view
macro_rules! impl_op_assign {
    ($type:tt, $rhs:ty, $trait:ident, $fn:ident, $op:tt) => {
        impl_helper!($trait<$rhs>, $type, {
            fn $fn(&mut self, rhs: $rhs) {
                for i in 0..self.len() {
                    self[i] $op rhs[i];
                }
            }
        });
    };
}

impl_op_assign!(SundialsVector, SundialsVector, SubAssign, sub_assign, -=);
impl_op_assign!(SundialsVector, &SundialsVector, SubAssign, sub_assign, -=);
impl_op_assign!(SundialsVector, SundialsVectorView<'_>, SubAssign, sub_assign, -=);
impl_op_assign!(SundialsVector, &SundialsVectorView<'_>, SubAssign, sub_assign, -=);
impl_op_assign!(SundialsVectorViewMut, SundialsVector, SubAssign, sub_assign, -=);
impl_op_assign!(SundialsVectorViewMut, &SundialsVector, SubAssign, sub_assign, -=);
impl_op_assign!(SundialsVectorViewMut, SundialsVectorView<'_>, SubAssign, sub_assign, -=);
impl_op_assign!(SundialsVectorViewMut, &SundialsVectorView<'_>, SubAssign, sub_assign, -=);

impl_op_assign!(SundialsVector, SundialsVector, AddAssign, add_assign, +=);
impl_op_assign!(SundialsVector, &SundialsVector, AddAssign, add_assign, +=);
impl_op_assign!(SundialsVector, SundialsVectorView<'_>, AddAssign, add_assign, +=);
impl_op_assign!(SundialsVector, &SundialsVectorView<'_>, AddAssign, add_assign, +=);
impl_op_assign!(SundialsVectorViewMut, SundialsVector, AddAssign, add_assign, +=);
impl_op_assign!(SundialsVectorViewMut, &SundialsVector, AddAssign, add_assign, +=);
impl_op_assign!(SundialsVectorViewMut, SundialsVectorView<'_>, AddAssign, add_assign, +=);
impl_op_assign!(SundialsVectorViewMut, &SundialsVectorView<'_>, AddAssign, add_assign, +=);

// owned binop by vector -> owned
macro_rules! impl_binop_owned {
    ($type:tt, $rhs:ty, $trait:ident, $fn:ident, $op:tt) => {
        impl_helper!($trait<$rhs>, $type, {
            type Output = SundialsVector;
            fn $fn(mut self, rhs: $rhs) -> Self::Output {
                self $op rhs;
                self
            }
        });
    };
}

impl_binop_owned!(SundialsVector, SundialsVector, Sub, sub, -=);
impl_binop_owned!(SundialsVector, &SundialsVector, Sub, sub, -=);
impl_binop_owned!(SundialsVector, &SundialsVectorView<'_>, Sub, sub, -=);
impl_binop_owned!(SundialsVector, SundialsVectorView<'_>, Sub, sub, -=);
impl_binop_owned!(SundialsVector, SundialsVector, Add, add, +=);
impl_binop_owned!(SundialsVector, &SundialsVector, Add, add, +=);
impl_binop_owned!(SundialsVector, &SundialsVectorView<'_>, Add, add, +=);
impl_binop_owned!(SundialsVector, SundialsVectorView<'_>, Add, add, +=);

// binop alloc owned
macro_rules! impl_binop_alloc_owned {
    ($type:tt, $rhs:ty, $trait:ident, $fn:ident, $op:tt) => {
        impl_helper!($trait<$rhs>, $type, {
            type Output = SundialsVector;
            fn $fn(self, rhs: $rhs) -> Self::Output {
                let mut z = SundialsVector::new_serial(self.len());
                z.copy_from_view(&self);
                z $op rhs;
                z
            }
        });
    };
}

impl_binop_alloc_owned!(SundialsVectorView, &SundialsVector, Sub, sub, -=);
impl_binop_alloc_owned!(SundialsVectorView, SundialsVectorView<'_>, Sub, sub, -=);
impl_binop_alloc_owned!(SundialsVectorView, &SundialsVectorView<'_>, Sub, sub, -=);
impl_binop_alloc_owned!(SundialsVectorView, &SundialsVector, Add, add, +=);
impl_binop_alloc_owned!(SundialsVectorView, SundialsVectorView<'_>, Add, add, +=);
impl_binop_alloc_owned!(SundialsVectorView, &SundialsVectorView<'_>, Add, add, +=);

// view binop by owned vector
macro_rules! impl_add_view_owned {
    ($type:tt, $rhs:ty) => {
        impl_helper!(Add<$rhs>, $type, {
            type Output = SundialsVector;
            fn add(self, mut rhs: $rhs) -> Self::Output {
                rhs += self;
                rhs
            }
        });
    };
}

impl_add_view_owned!(SundialsVectorView, SundialsVector);

macro_rules! impl_sub_view_owned {
    ($type:tt, $rhs:ty) => {
        impl_helper!(Sub<$rhs>, $type, {
            type Output = SundialsVector;
            fn sub(self, mut rhs: $rhs) -> Self::Output {
                rhs -= self;
                rhs *= scale(-1.0);
                rhs
            }
        });
    };
}

impl_sub_view_owned!(SundialsVectorView, SundialsVector);

impl<'a> VectorViewMut<'a> for SundialsVectorViewMut<'a> {
    type Owned = SundialsVector;
    type View = SundialsVectorView<'a>;
    fn abs(&self) -> Self::Owned {
        let z = SundialsVector::new_serial(self.len());
        unsafe { N_VAbs(self.sundials_vector(), z.sundials_vector()) }
        z
    }
    fn copy_from(&mut self, other: &Self::Owned) {
        unsafe { N_VScale(1.0, other.sundials_vector(), self.sundials_vector()) }
    }
    fn copy_from_view(&mut self, other: &Self::View) {
        unsafe { N_VScale(1.0, other.sundials_vector(), self.sundials_vector()) }
    }
}

impl<'a> VectorView<'a> for SundialsVectorView<'a> {
    type Owned = SundialsVector;
    fn abs(&self) -> Self::Owned {
        let z = SundialsVector::new_serial(self.len());
        unsafe { N_VAbs(self.sundials_vector(), z.sundials_vector()) }
        z
    }
    fn into_owned(self) -> Self::Owned {
        let mut z = SundialsVector::new_serial(self.len());
        z.copy_from_view(&self);
        z
    }
}

impl VectorIndex for SundialsIndexVector {
    fn zeros(len: IndexType) -> Self {
        Self(vec![0; len])
    }
    fn len(&self) -> IndexType {
        self.0.len() as IndexType
    }
}

impl Vector for SundialsVector {
    type View<'a> = SundialsVectorView<'a> where Self: 'a;
    type ViewMut<'a> = SundialsVectorViewMut<'a> where Self: 'a;
    type Index = SundialsIndexVector;
    fn len(&self) -> IndexType {
        unsafe { N_VGetLength_Serial(self.sundials_vector()) as IndexType }
    }
    fn norm(&self) -> Self::T {
        let ones = SundialsVector::from_element(self.len(), 1.0);
        unsafe { N_VWL2Norm_Serial(self.sundials_vector(), ones.sundials_vector()) }
    }
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn abs(&self) -> Self {
        let z = SundialsVector::new_clone(self);
        unsafe { N_VAbs(self.sundials_vector(), z.sundials_vector()) }
        z
    }
    fn add_scalar_mut(&mut self, scalar: Self::T) {
        unsafe { N_VAddConst(self.sundials_vector(), scalar, self.sundials_vector()) }
    }
    fn as_view(&self) -> Self::View<'_> {
        SundialsVectorView(self)
    }
    fn as_view_mut(&mut self) -> Self::ViewMut<'_> {
        SundialsVectorViewMut(self)
    }
    fn axpy(&mut self, alpha: Self::T, x: &Self, beta: Self::T) {
        unsafe {
            N_VLinearSum(
                alpha,
                x.sundials_vector(),
                beta,
                self.sundials_vector(),
                self.sundials_vector(),
            )
        };
    }
    fn component_div_assign(&mut self, other: &Self) {
        unsafe {
            N_VDiv(
                self.sundials_vector(),
                other.sundials_vector(),
                self.sundials_vector(),
            )
        };
    }
    fn component_mul_assign(&mut self, other: &Self) {
        unsafe {
            N_VProd(
                self.sundials_vector(),
                other.sundials_vector(),
                self.sundials_vector(),
            )
        };
    }
    fn copy_from(&mut self, other: &Self) {
        unsafe { N_VScale(1.0, other.sundials_vector(), self.sundials_vector()) }
    }
    fn copy_from_view(&mut self, other: &Self::View<'_>) {
        unsafe { N_VScale(1.0, other.sundials_vector(), self.sundials_vector()) }
    }
    fn exp(&self) -> Self {
        let mut z = SundialsVector::new_clone(self);
        for i in 0..self.len() {
            z[i] = self[i].exp();
        }
        z
    }
    fn filter_indices<F: Fn(Self::T) -> bool>(&self, f: F) -> Self::Index {
        let mut indices = vec![];
        for i in 0..self.len() {
            if f(self[i]) {
                indices.push(i);
            }
        }
        SundialsIndexVector(indices)
    }
    fn from_element(nstates: usize, value: Self::T) -> Self {
        let v = SundialsVector::new_serial(nstates);
        unsafe { N_VConst(value, v.sundials_vector()) };
        v
    }
    fn from_vec(vec: Vec<Self::T>) -> Self {
        let mut v = SundialsVector::new_serial(vec.len());
        for (i, &x) in vec.iter().enumerate() {
            v[i] = x;
        }
        v
    }
    fn gather_from(&mut self, other: &Self, indices: &Self::Index) {
        for i in 0..indices.len() {
            self[i] = other[indices[i]];
        }
    }
    fn scatter_from(&mut self, other: &Self, indices: &Self::Index) {
        for i in 0..indices.len() {
            self[indices[i]] = other[i];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indexing() {
        let mut v = SundialsVector::new_serial(2);
        v[0] = 1.0;
        v[1] = 2.0;
        assert_eq!(v[0], 1.0);
        assert_eq!(v[1], 2.0);
    }

    #[test]
    fn test_add_sub_ops() {
        let mut v = SundialsVector::new_serial(2);
        v[0] = 1.0;
        v[1] = 2.0;
        let v2 = v.clone();
        let v3 = v + &v2;
        assert_eq!(v3[0], 2.0);
        assert_eq!(v3[1], 4.0);
        let v4 = v3 - v2;
        assert_eq!(v4[0], 1.0);
        assert_eq!(v4[1], 2.0);
    }

    #[test]
    fn test_mul_div_ops() {
        let mut v = SundialsVector::new_serial(2);
        v[0] = 1.0;
        v[1] = 2.0;
        let v2 = v * scale(2.0);
        assert_eq!(v2[0], 2.0);
        assert_eq!(v2[1], 4.0);
        let v3 = v2 / scale(2.0);
        assert_eq!(v3[0], 1.0);
        assert_eq!(v3[1], 2.0);
    }

    #[test]
    fn test_abs() {
        let mut v = SundialsVector::new_serial(2);
        v[0] = -1.0;
        v[1] = 2.0;
        let v2 = v.abs();
        assert_eq!(v2[0], 1.0);
        assert_eq!(v2[1], 2.0);
    }

    #[test]
    fn test_axpy() {
        let mut v = SundialsVector::new_serial(2);
        v[0] = 1.0;
        v[1] = 2.0;
        let mut v2 = SundialsVector::new_serial(2);
        v2[0] = 2.0;
        v2[1] = 3.0;
        v.axpy(2.0, &v2, 1.0);
        assert_eq!(v[0], 5.0);
        assert_eq!(v[1], 8.0);
    }

    #[test]
    fn test_component_mul_div() {
        let mut v = SundialsVector::new_serial(2);
        v[0] = 1.0;
        v[1] = 2.0;
        let mut v2 = SundialsVector::new_serial(2);
        v2[0] = 2.0;
        v2[1] = 3.0;
        v.component_mul_assign(&v2);
        assert_eq!(v[0], 2.0);
        assert_eq!(v[1], 6.0);
        v.component_div_assign(&v2);
        assert_eq!(v[0], 1.0);
        assert_eq!(v[1], 2.0);
    }

    #[test]
    fn test_copy_from() {
        let mut v = SundialsVector::new_serial(2);
        v[0] = 1.0;
        v[1] = 2.0;
        let mut v2 = SundialsVector::new_serial(2);
        v2.copy_from(&v);
        assert_eq!(v2[0], 1.0);
        assert_eq!(v2[1], 2.0);
    }

    #[test]
    fn test_exp() {
        let mut v = SundialsVector::new_serial(2);
        v[0] = 1.0;
        v[1] = 2.0;
        let v2 = v.exp();
        assert_eq!(v2[0], 1.0_f64.exp());
        assert_eq!(v2[1], 2.0_f64.exp());
    }

    #[test]
    fn test_filter_indices() {
        let mut v = SundialsVector::new_serial(2);
        v[0] = 1.0;
        v[1] = 2.0;
        let indices = v.filter_indices(|x| x > 1.0);
        assert_eq!(indices.len(), 1);
        assert_eq!(indices[0], 1);
    }

    #[test]
    fn test_gather_scatter() {
        let mut v = SundialsVector::new_serial(3);
        v[0] = 1.0;
        v[1] = 2.0;
        v[2] = 3.0;
        let mut v2 = SundialsVector::new_serial(2);
        v2.gather_from(&v, &SundialsIndexVector(vec![0, 2]));
        assert_eq!(v2[0], 1.0);
        assert_eq!(v2[1], 3.0);
        v2[0] = 4.0;
        v2[1] = 5.0;
        v.scatter_from(&v2, &SundialsIndexVector(vec![0, 2]));
        assert_eq!(v[0], 4.0);
        assert_eq!(v[1], 2.0);
        assert_eq!(v[2], 5.0);
    }

    #[test]
    fn test_zeros() {
        let v = SundialsIndexVector::zeros(1);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0], 0);
        let v = SundialsVector::zeros(1);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0], 0.);
    }

    #[test]
    fn test_from_element() {
        let v = SundialsVector::from_element(2, 1.0);
        assert_eq!(v[0], 1.0);
        assert_eq!(v[1], 1.0);
    }

    #[test]
    fn test_from_vec() {
        let v = SundialsVector::from_vec(vec![1.0, 2.0]);
        assert_eq!(v[0], 1.0);
        assert_eq!(v[1], 2.0);
    }

    #[test]
    fn test_norm() {
        let mut v = SundialsVector::new_serial(2);
        v[0] = 1.0;
        v[1] = 2.0;
        let norm = v.norm();
        assert_eq!(norm, (1.0_f64.powi(2) + 2.0_f64.powi(2)).sqrt());
    }
}
