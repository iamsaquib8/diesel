use byteorder::{NetworkEndian, ReadBytesExt, WriteBytesExt};
use std::fmt;
use std::io::Write;

use deserialize::{self, FromSql};
use pg::{Pg, PgMetadataLookup, PgTypeMetadata, PgValue};
use serialize::{self, IsNull, Output, ToSql};
use sql_types::{Array, HasSqlType, Nullable};

impl<T> HasSqlType<Array<T>> for Pg
where
    Pg: HasSqlType<T>,
{
    fn metadata(lookup: &PgMetadataLookup) -> PgTypeMetadata {
        PgTypeMetadata {
            oid: <Pg as HasSqlType<T>>::metadata(lookup).array_oid,
            array_oid: 0,
        }
    }
}

impl<T, ST> FromSql<Array<ST>, Pg> for Vec<T>
where
    T: FromSql<ST, Pg>,
{
    fn from_sql(value: Option<PgValue<'_>>) -> deserialize::Result<Self> {
        let value = not_none!(value);
        let mut bytes = value.as_bytes();
        let num_dimensions = bytes.read_i32::<NetworkEndian>()?;
        let has_null = bytes.read_i32::<NetworkEndian>()? != 0;
        let _oid = bytes.read_i32::<NetworkEndian>()?;

        if num_dimensions == 0 {
            return Ok(Vec::new());
        }

        let num_elements = bytes.read_i32::<NetworkEndian>()?;
        let _lower_bound = bytes.read_i32::<NetworkEndian>()?;

        if num_dimensions != 1 {
            return Err("multi-dimensional arrays are not supported".into());
        }

        (0..num_elements)
            .map(|_| {
                let elem_size = bytes.read_i32::<NetworkEndian>()?;
                if has_null && elem_size == -1 {
                    T::from_sql(None)
                } else {
                    let (elem_bytes, new_bytes) = bytes.split_at(elem_size as usize);
                    bytes = new_bytes;
                    T::from_sql(Some(PgValue::new(elem_bytes, value.get_oid())))
                }
            })
            .collect()
    }
}

use expression::bound::Bound;
use expression::AsExpression;

macro_rules! array_as_expression {
    ($ty:ty, $sql_type:ty) => {
        impl<'a, 'b, ST, T> AsExpression<$sql_type> for $ty {
            type Expression = Bound<$sql_type, Self>;

            fn as_expression(self) -> Self::Expression {
                Bound::new(self)
            }
        }
    };
}

array_as_expression!(&'a [T], Array<ST>);
array_as_expression!(&'a [T], Nullable<Array<ST>>);
array_as_expression!(&'a &'b [T], Array<ST>);
array_as_expression!(&'a &'b [T], Nullable<Array<ST>>);
array_as_expression!(Vec<T>, Array<ST>);
array_as_expression!(Vec<T>, Nullable<Array<ST>>);
array_as_expression!(&'a Vec<T>, Array<ST>);
array_as_expression!(&'a Vec<T>, Nullable<Array<ST>>);
array_as_expression!(&'a &'b Vec<T>, Array<ST>);
array_as_expression!(&'a &'b Vec<T>, Nullable<Array<ST>>);

impl<ST, T> ToSql<Array<ST>, Pg> for [T]
where
    Pg: HasSqlType<ST>,
    T: ToSql<ST, Pg>,
{
    fn to_sql<W: Write>(&self, out: &mut Output<W, Pg>) -> serialize::Result {
        let num_dimensions = 1;
        out.write_i32::<NetworkEndian>(num_dimensions)?;
        let flags = 0;
        out.write_i32::<NetworkEndian>(flags)?;
        let element_oid = Pg::metadata(out.metadata_lookup()).oid;
        out.write_u32::<NetworkEndian>(element_oid)?;
        out.write_i32::<NetworkEndian>(self.len() as i32)?;
        let lower_bound = 1;
        out.write_i32::<NetworkEndian>(lower_bound)?;

        let mut buffer = out.with_buffer(Vec::new());
        for elem in self.iter() {
            let is_null = elem.to_sql(&mut buffer)?;
            if let IsNull::No = is_null {
                out.write_i32::<NetworkEndian>(buffer.len() as i32)?;
                out.write_all(&buffer)?;
                buffer.clear();
            } else {
                // https://github.com/postgres/postgres/blob/82f8107b92c9104ec9d9465f3f6a4c6dab4c124a/src/backend/utils/adt/arrayfuncs.c#L1461
                out.write_i32::<NetworkEndian>(-1)?;
            }
        }

        Ok(IsNull::No)
    }
}

impl<ST, T> ToSql<Nullable<Array<ST>>, Pg> for [T]
where
    [T]: ToSql<Array<ST>, Pg>,
{
    fn to_sql<W: Write>(&self, out: &mut Output<W, Pg>) -> serialize::Result {
        ToSql::<Array<ST>, Pg>::to_sql(self, out)
    }
}

impl<ST, T> ToSql<Array<ST>, Pg> for Vec<T>
where
    [T]: ToSql<Array<ST>, Pg>,
    T: fmt::Debug,
{
    fn to_sql<W: Write>(&self, out: &mut Output<W, Pg>) -> serialize::Result {
        (self as &[T]).to_sql(out)
    }
}

impl<ST, T> ToSql<Nullable<Array<ST>>, Pg> for Vec<T>
where
    Vec<T>: ToSql<Array<ST>, Pg>,
{
    fn to_sql<W: Write>(&self, out: &mut Output<W, Pg>) -> serialize::Result {
        ToSql::<Array<ST>, Pg>::to_sql(self, out)
    }
}
