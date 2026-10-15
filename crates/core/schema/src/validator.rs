use std::{borrow::Cow, cmp::min, collections::HashSet, fmt::Write};

use serde_json::{Map, Value};

use crate::{util::*, *};

macro_rules! prop {
    ($prop:expr) => {
        InstanceToken::Prop(Cow::Borrowed($prop))
    };
}

macro_rules! item {
    ($item:expr) => {
        InstanceToken::Item($item)
    };
}

pub(crate) fn validate<'s, 'v>(
    v: &'v Value,
    schema: &'s Schema,
    schemas: &'s Schemas,
    bool_result: bool,
) -> Result<(), ValidationError<'s, 'v>> {
    let scope = Scope {
        sch: schema.idx,
        ref_kw: None,
        vid: 0,
        parent: None,
    };
    let mut vloc = Vec::with_capacity(8);
    let result = Validator {
        v,
        vloc: &mut vloc,
        schema,
        schemas,
        scope,
        uneval: Uneval::from(v, schema, false),
        errors: vec![],
        bool_result,
    }
    .validate();
    match result {
        Err(err) => {
            let mut e = ValidationError {
                schema_url: &schema.loc,
                instance_location: InstanceLocation::new(),
                kind: ErrorKind::Schema { url: &schema.loc },
                causes: vec![],
            };
            if let ErrorKind::Group = err.kind {
                e.causes = err.causes;
            } else {
                e.causes.push(err);
            }
            Err(e)
        }
        Ok(_) => Ok(()),
    }
}

macro_rules! kind {
    ($kind:ident, $name:ident: $value:expr) => {
        ErrorKind::$kind { $name: $value }
    };
    ($kind:ident, $got:expr, $want:expr) => {
        ErrorKind::$kind {
            got: $got,
            want: $want,
        }
    };
    ($kind:ident, $got:expr, $want:expr, $err:expr) => {
        ErrorKind::$kind {
            got: $got,
            want: $want,
            err: $err,
        }
    };
    ($kind: ident) => {
        ErrorKind::$kind
    };
}

struct Validator<'v, 's, 'd, 'e> {
    v: &'v Value,
    vloc: &'e mut Vec<InstanceToken<'v>>,
    schema: &'s Schema,
    schemas: &'s Schemas,
    scope: Scope<'d>,
    uneval: Uneval<'v>,
    errors: Vec<ValidationError<'s, 'v>>,
    bool_result: bool,
}

impl<'v, 's> Validator<'v, 's, '_, '_> {
    fn validate(mut self) -> Result<Uneval<'v>, ValidationError<'s, 'v>> {
        let s = self.schema;
        let v = self.v;

        if let Some(b) = s.boolean {
            return match b {
                false => Err(self.error(kind!(FalseSchema))),
                true => Ok(self.uneval),
            };
        }

        if let Some(scp) = self.scope.check_cycle() {
            let kind = ErrorKind::RefCycle {
                url: &self.schema.loc,
                kw_loc1: self.kw_loc(&self.scope),
                kw_loc2: self.kw_loc(scp),
            };
            return Err(self.error(kind));
        }

        if !s.types.is_empty() {
            let v_type = Type::of(v);
            let matched = s.types.contains(v_type)
                || (s.types.contains(Type::Integer)
                    && (s.draft_version != 4
                        || v.as_number().is_some_and(|n| n.is_i64() || n.is_u64()))
                    && is_integer(v));
            if !matched {
                return Err(self.error(kind!(Type, v_type, s.types)));
            }
        }

        if let Some(c) = &s.constant
            && !equals(v, c)
        {
            return Err(self.error(kind!(Const, want: c)));
        }

        if let Some(Enum { types, values }) = &s.enum_
            && (!types.contains(Type::of(v)) || !values.iter().any(|e| equals(e, v)))
        {
            return Err(self.error(kind!(Enum, want: values)));
        }

        if let Some(format) = &s.format
            && let Err(e) = (format.func)(v)
        {
            self.add_error(kind!(Format, Cow::Borrowed(v), format.name, e));
        }

        if let Some(ref_) = s.ref_ {
            let result = self.validate_ref(ref_, "$ref");
            if s.draft_version < 2019 {
                return result.map(|_| self.uneval);
            }
            self.errors.extend(result.err());
        }

        match v {
            Value::Object(obj) => self.obj_validate(obj),
            Value::Array(arr) => self.arr_validate(arr),
            Value::String(str) => self.str_validate(str),
            Value::Number(num) => self.num_validate(num),
            _ => {}
        }

        if self.errors.is_empty() || !self.bool_result {
            if s.draft_version >= 2019 {
                self.refs_validate();
            }
            self.cond_validate();
            if s.draft_version >= 2019 {
                self.uneval_validate();
            }
        }

        match self.errors.len() {
            0 => Ok(self.uneval),
            1 => Err(self.errors.remove(0)),
            _ => {
                let mut e = self.error(kind!(Group));
                e.causes = self.errors;
                Err(e)
            }
        }
    }
}

impl<'v> Validator<'v, '_, '_, '_> {
    fn obj_validate(&mut self, obj: &'v Map<String, Value>) {
        let s = self.schema;
        macro_rules! add_err {
            ($result:expr) => {
                if let Err(e) = $result {
                    self.errors.push(e);
                }
            };
        }

        if let Some(min) = &s.min_properties
            && crate::numbers::compare_count(obj.len(), min).is_lt()
        {
            self.add_error(kind!(MinProperties, obj.len(), min));
        }

        if let Some(max) = &s.max_properties
            && crate::numbers::compare_count(obj.len(), max).is_gt()
        {
            self.add_error(kind!(MaxProperties, obj.len(), max));
        }

        if !s.required.is_empty()
            && let Some(missing) = self.find_missing(obj, &s.required)
        {
            self.add_error(kind!(Required, want: missing));
        }

        if self.bool_result && !self.errors.is_empty() {
            return;
        }

        for (prop, dep) in &s.dependencies {
            if obj.contains_key(prop) {
                match dep {
                    Dependency::Props(required) => {
                        if let Some(missing) = self.find_missing(obj, required) {
                            self.add_error(ErrorKind::Dependency { prop, missing });
                        }
                    }
                    Dependency::SchemaRef(sch) => {
                        add_err!(self.validate_self(*sch));
                    }
                }
            }
        }

        let mut additional_props = vec![];
        for (pname, pvalue) in obj {
            if self.bool_result && !self.errors.is_empty() {
                return;
            }
            let mut evaluated = false;

            if let Some(sch) = s.properties.get(pname) {
                evaluated = true;
                add_err!(self.validate_val(*sch, pvalue, prop!(pname)));
            }

            for (regex, sch) in &s.pattern_properties {
                if regex.is_match(pname) {
                    evaluated = true;
                    add_err!(self.validate_val(*sch, pvalue, prop!(pname)));
                }
            }

            if !evaluated && let Some(additional) = &s.additional_properties {
                evaluated = true;
                match additional {
                    Additional::Bool(allowed) => {
                        if !allowed {
                            additional_props.push(pname.into());
                        }
                    }
                    Additional::SchemaRef(sch) => {
                        add_err!(self.validate_val(*sch, pvalue, prop!(pname)));
                    }
                }
            }

            if evaluated {
                self.uneval.props.remove(pname);
            }
        }
        if !additional_props.is_empty() {
            self.add_error(kind!(AdditionalProperties, got: additional_props));
        }

        if s.draft_version == 4 {
            return;
        }

        if let Some(sch) = &s.property_names {
            for pname in obj.keys() {
                let v = Value::String(pname.to_owned());
                if let Err(mut e) = self.schemas.validate(&v, *sch) {
                    e.schema_url = &s.loc;
                    e.kind = ErrorKind::PropertyName {
                        prop: pname.to_owned(),
                    };
                    self.errors.push(e.clone_static());
                }
            }
        }

        if s.draft_version == 6 {
            return;
        }

        for (pname, sch) in &s.dependent_schemas {
            if obj.contains_key(pname) {
                add_err!(self.validate_self(*sch));
            }
        }

        for (prop, required) in &s.dependent_required {
            if obj.contains_key(prop)
                && let Some(missing) = self.find_missing(obj, required)
            {
                self.add_error(ErrorKind::DependentRequired { prop, missing });
            }
        }
    }

    fn arr_validate(&mut self, arr: &'v Vec<Value>) {
        let s = self.schema;
        let len = arr.len();
        macro_rules! add_err {
            ($result:expr) => {
                if let Err(e) = $result {
                    self.errors.push(e);
                }
            };
        }

        if let Some(min) = &s.min_items
            && crate::numbers::compare_count(len, min).is_lt()
        {
            self.add_error(kind!(MinItems, len, min));
        }

        if let Some(max) = &s.max_items
            && crate::numbers::compare_count(len, max).is_gt()
        {
            self.add_error(kind!(MaxItems, len, max));
        }

        if len > 1
            && s.unique_items
            && let Some((i, j)) = duplicates(arr)
        {
            self.add_error(kind!(UniqueItems, got: [i, j]));
        }

        if s.draft_version < 2020 {
            let mut evaluated = 0;

            if let Some(items) = &s.items {
                match items {
                    Items::SchemaRef(sch) => {
                        for (i, item) in arr.iter().enumerate() {
                            add_err!(self.validate_val(*sch, item, item!(i)));
                        }
                        evaluated = len;
                        debug_assert!(self.uneval.items.is_empty());
                    }
                    Items::SchemaRefs(list) => {
                        for (i, (item, sch)) in arr.iter().zip(list).enumerate() {
                            add_err!(self.validate_val(*sch, item, item!(i)));
                        }
                        evaluated = min(list.len(), len);
                    }
                }
            }

            if let Some(additional) = &s.additional_items {
                match additional {
                    Additional::Bool(allowed) => {
                        if !allowed && evaluated != len {
                            self.add_error(kind!(AdditionalItems, got: len - evaluated));
                        }
                    }
                    Additional::SchemaRef(sch) => {
                        for (i, item) in arr[evaluated..].iter().enumerate() {
                            add_err!(self.validate_val(*sch, item, item!(i)));
                        }
                    }
                }
                debug_assert!(self.uneval.items.is_empty());
            }
        } else {
            for (i, (sch, item)) in s.prefix_items.iter().zip(arr).enumerate() {
                add_err!(self.validate_val(*sch, item, item!(i)));
            }

            if let Some(sch) = &s.items2020 {
                let evaluated = min(s.prefix_items.len(), len);
                for (i, item) in arr[evaluated..].iter().enumerate() {
                    add_err!(self.validate_val(*sch, item, item!(i)));
                }
                debug_assert!(self.uneval.items.is_empty());
            }
        }

        if let Some(sch) = &s.contains {
            let mut matched = vec![];
            let mut errors = vec![];

            for (i, item) in arr.iter().enumerate() {
                if let Err(e) = self.validate_val(*sch, item, item!(i)) {
                    errors.push(e);
                } else {
                    matched.push(i);
                    if s.draft_version >= 2020 {
                        self.uneval.items.remove(&i);
                    }
                }
            }

            if let Some(min) = &s.min_contains {
                if crate::numbers::compare_count(matched.len(), min).is_lt() {
                    let mut e = self.error(kind!(MinContains, matched.clone(), min));
                    e.causes = errors;
                    self.errors.push(e);
                }
            } else if matched.is_empty() {
                let mut e = self.error(kind!(Contains));
                e.causes = errors;
                self.errors.push(e);
            }

            if let Some(max) = &s.max_contains
                && crate::numbers::compare_count(matched.len(), max).is_gt()
            {
                self.add_error(kind!(MaxContains, matched, max));
            }
        }
    }

    fn str_validate(&mut self, str: &'v String) {
        let s = self.schema;
        let mut len = None;

        if let Some(min) = &s.min_length {
            let len = len.get_or_insert_with(|| str.chars().count());
            if crate::numbers::compare_count(*len, min).is_lt() {
                self.add_error(kind!(MinLength, *len, min));
            }
        }

        if let Some(max) = &s.max_length {
            let len = len.get_or_insert_with(|| str.chars().count());
            if crate::numbers::compare_count(*len, max).is_gt() {
                self.add_error(kind!(MaxLength, *len, max));
            }
        }

        if let Some(regex) = &s.pattern
            && !regex.is_match(str)
        {
            self.add_error(kind!(Pattern, str.into(), regex.as_str()));
        }

        if s.draft_version == 6 {
            return;
        }

        let mut decoded = Some(Cow::from(str.as_bytes()));
        if let Some(decoder) = &s.content_encoding {
            match (decoder.func)(str) {
                Ok(bytes) => decoded = Some(Cow::from(bytes)),
                Err(err) => {
                    decoded = None;
                    self.add_error(ErrorKind::ContentEncoding {
                        want: decoder.name,
                        err,
                    })
                }
            }
        }

        let mut deserialized = None;
        if let (Some(mt), Some(decoded)) = (&s.content_media_type, decoded) {
            match (mt.func)(decoded.as_ref(), s.content_schema.is_some()) {
                Ok(des) => deserialized = des,
                Err(e) => {
                    self.add_error(kind!(ContentMediaType, decoded.into(), mt.name, e));
                }
            }
        }

        if let (Some(sch), Some(v)) = (s.content_schema, deserialized)
            && let Err(mut e) = self.schemas.validate(&v, sch)
        {
            e.schema_url = &s.loc;
            e.kind = kind!(ContentSchema);
            self.errors.push(e.clone_static());
        }
    }

    fn num_validate(&mut self, num: &'v Number) {
        let s = self.schema;

        if let Some(min) = &s.minimum
            && crate::numbers::compare(num, min).is_lt()
        {
            self.add_error(kind!(Minimum, Cow::Borrowed(num), min));
        }
        if let Some(max) = &s.maximum
            && crate::numbers::compare(num, max).is_gt()
        {
            self.add_error(kind!(Maximum, Cow::Borrowed(num), max));
        }
        if let Some(min) = &s.exclusive_minimum
            && !crate::numbers::compare(num, min).is_gt()
        {
            self.add_error(kind!(ExclusiveMinimum, Cow::Borrowed(num), min));
        }
        if let Some(max) = &s.exclusive_maximum
            && !crate::numbers::compare(num, max).is_lt()
        {
            self.add_error(kind!(ExclusiveMaximum, Cow::Borrowed(num), max));
        }
        if let Some(divisor) = &s.multiple_of
            && !crate::numbers::multiple_of(num, divisor)
        {
            self.add_error(kind!(MultipleOf, Cow::Borrowed(num), divisor));
        }
    }
}

impl<'v, 's> Validator<'v, 's, '_, '_> {
    fn refs_validate(&mut self) {
        let s = self.schema;
        macro_rules! add_err {
            ($result:expr) => {
                if let Err(e) = $result {
                    self.errors.push(e);
                }
            };
        }

        if let Some(mut sch) = s.recursive_ref {
            if self.schemas.get(sch).recursive_anchor {
                sch = self.resolve_recursive_anchor(sch);
            }
            add_err!(self.validate_ref(sch, "$recursiveRef"));
        }

        if let Some(dref) = &s.dynamic_ref {
            let mut sch = dref.sch;
            if let Some(anchor) = &dref.anchor
                && self.schemas.get(sch).dynamic_anchor == dref.anchor
            {
                sch = self.resolve_dynamic_anchor(anchor, sch);
            }
            add_err!(self.validate_ref(sch, "$dynamicRef"));
        }
    }

    fn validate_ref(
        &mut self,
        sch: SchemaIndex,
        kw: &'static str,
    ) -> Result<(), ValidationError<'s, 'v>> {
        if let Err(err) = self._validate_self(sch, kw.into(), false) {
            let url = &self.schemas.get(sch).loc;
            let mut ref_err = self.error(ErrorKind::Reference { kw, url });
            if let ErrorKind::Group = err.kind {
                ref_err.causes = err.causes;
            } else {
                ref_err.causes.push(err);
            }
            return Err(ref_err);
        }
        Ok(())
    }

    fn resolve_recursive_anchor(&self, fallback: SchemaIndex) -> SchemaIndex {
        let mut sch = fallback;
        let mut scope = &self.scope;
        loop {
            let scope_sch = self.schemas.get(scope.sch);
            let base_sch = self.schemas.get(scope_sch.resource);
            if base_sch.recursive_anchor {
                sch = scope.sch
            }
            if let Some(parent) = scope.parent {
                scope = parent;
            } else {
                return sch;
            }
        }
    }

    fn resolve_dynamic_anchor(&self, name: &String, fallback: SchemaIndex) -> SchemaIndex {
        let mut sch = fallback;
        let mut scope = &self.scope;
        loop {
            let scope_sch = self.schemas.get(scope.sch);
            let base_sch = self.schemas.get(scope_sch.resource);
            debug_assert_eq!(base_sch.idx, base_sch.resource);
            if let Some(dsch) = base_sch.dynamic_anchors.get(name) {
                sch = *dsch
            }
            if let Some(parent) = scope.parent {
                scope = parent;
            } else {
                return sch;
            }
        }
    }
}

impl Validator<'_, '_, '_, '_> {
    fn cond_validate(&mut self) {
        let s = self.schema;
        macro_rules! add_err {
            ($result:expr) => {
                if let Err(e) = $result {
                    self.errors.push(e);
                }
            };
        }

        if let Some(not) = s.not
            && self._validate_self(not, None, true).is_ok()
        {
            self.add_error(kind!(Not));
        }

        if !s.all_of.is_empty() {
            let mut errors = vec![];
            for sch in &s.all_of {
                if let Err(e) = self.validate_self(*sch) {
                    errors.push(e);
                    if self.bool_result {
                        break;
                    }
                }
            }
            if !errors.is_empty() {
                self.add_errors(errors, kind!(AllOf));
            }
        }

        if !s.any_of.is_empty() {
            let mut matched = false;
            let mut errors = vec![];
            for sch in &s.any_of {
                match self.validate_self(*sch) {
                    Ok(_) => {
                        matched = true;

                        if self.uneval.is_empty() {
                            break;
                        }
                    }
                    Err(e) => errors.push(e),
                }
            }
            if !matched {
                self.add_errors(errors, kind!(AnyOf));
            }
        }

        if !s.one_of.is_empty() {
            let mut matched = None;
            let mut errors = vec![];
            for (i, sch) in s.one_of.iter().enumerate() {
                if let Err(e) = self._validate_self(*sch, None, matched.is_some()) {
                    if matched.is_none() {
                        errors.push(e);
                    }
                } else {
                    match matched {
                        None => _ = matched.replace(i),
                        Some(prev) => {
                            self.add_error(ErrorKind::OneOf(Some((prev, i))));
                            break;
                        }
                    }
                }
            }
            if matched.is_none() {
                self.add_errors(errors, ErrorKind::OneOf(None));
            }
        }

        if let Some(if_) = s.if_ {
            if self._validate_self(if_, None, true).is_ok() {
                if let Some(then) = s.then {
                    add_err!(self.validate_self(then));
                }
            } else if let Some(else_) = s.else_ {
                add_err!(self.validate_self(else_));
            }
        }
    }
}

impl Validator<'_, '_, '_, '_> {
    fn uneval_validate(&mut self) {
        let s = self.schema;
        let v = self.v;
        macro_rules! add_err {
            ($result:expr) => {
                if let Err(e) = $result {
                    self.errors.push(e);
                }
            };
        }

        if let (Some(sch), Value::Object(obj)) = (s.unevaluated_properties, v) {
            let uneval = std::mem::take(&mut self.uneval);
            for pname in &uneval.props {
                if let Some(pvalue) = obj.get(*pname) {
                    add_err!(self.validate_val(sch, pvalue, prop!(pname)));
                }
            }
            self.uneval.props.clear();
        }

        if let (Some(sch), Value::Array(arr)) = (s.unevaluated_items, v) {
            let uneval = std::mem::take(&mut self.uneval);
            for i in &uneval.items {
                if let Some(pvalue) = arr.get(*i) {
                    add_err!(self.validate_val(sch, pvalue, item!(*i)));
                }
            }
            self.uneval.items.clear();
        }
    }
}

impl<'v, 's> Validator<'v, 's, '_, '_> {
    fn validate_val(
        &mut self,
        sch: SchemaIndex,
        v: &'v Value,
        token: InstanceToken<'v>,
    ) -> Result<(), ValidationError<'s, 'v>> {
        if self.vloc.len() == self.scope.vid {
            self.vloc.push(token);
        } else {
            self.vloc[self.scope.vid] = token;
        }
        let scope = self.scope.child(sch, None, self.scope.vid + 1);
        let schema = &self.schemas.get(sch);
        Validator {
            v,
            vloc: self.vloc,
            schema,
            schemas: self.schemas,
            scope,
            uneval: Uneval::from(v, schema, false),
            errors: vec![],
            bool_result: self.bool_result,
        }
        .validate()
        .map(|_| ())
    }

    fn _validate_self(
        &mut self,
        sch: SchemaIndex,
        ref_kw: Option<&'static str>,
        bool_result: bool,
    ) -> Result<(), ValidationError<'s, 'v>> {
        let scope = self.scope.child(sch, ref_kw, self.scope.vid);
        let schema = &self.schemas.get(sch);
        let result = Validator {
            v: self.v,
            vloc: self.vloc,
            schema,
            schemas: self.schemas,
            scope,
            uneval: Uneval::from(self.v, schema, !self.uneval.is_empty()),
            errors: vec![],
            bool_result: self.bool_result || bool_result,
        }
        .validate();
        if let Ok(reply) = &result {
            self.uneval.merge(reply);
        }
        result.map(|_| ())
    }

    #[inline(always)]
    fn validate_self(&mut self, sch: SchemaIndex) -> Result<(), ValidationError<'s, 'v>> {
        self._validate_self(sch, None, false)
    }
}

impl<'v, 's> Validator<'v, 's, '_, '_> {
    #[inline(always)]
    fn error(&self, kind: ErrorKind<'s, 'v>) -> ValidationError<'s, 'v> {
        if self.bool_result {
            return ValidationError {
                schema_url: &self.schema.loc,
                instance_location: InstanceLocation::new(),
                kind: ErrorKind::Group,
                causes: vec![],
            };
        }
        ValidationError {
            schema_url: &self.schema.loc,
            instance_location: self.instance_location(),
            kind,
            causes: vec![],
        }
    }

    #[inline(always)]
    fn add_error(&mut self, kind: ErrorKind<'s, 'v>) {
        self.errors.push(self.error(kind));
    }

    #[inline(always)]
    fn add_errors(&mut self, errors: Vec<ValidationError<'s, 'v>>, kind: ErrorKind<'s, 'v>) {
        if errors.len() == 1 {
            self.errors.extend(errors);
        } else {
            let mut err = self.error(kind);
            err.causes = errors;
            self.errors.push(err);
        }
    }

    fn kw_loc(&self, mut scope: &Scope) -> String {
        let mut loc = String::new();
        while let Some(parent) = scope.parent {
            if let Some(kw) = scope.ref_kw {
                loc.insert_str(0, kw);
                loc.insert(0, '/');
            } else {
                let cur = &self.schemas.get(scope.sch).loc;
                let parent = &self.schemas.get(parent.sch).loc;
                loc.insert_str(0, &cur[parent.len()..]);
            }
            scope = parent;
        }
        loc
    }

    fn find_missing(
        &self,
        obj: &'v Map<String, Value>,
        required: &'s [String],
    ) -> Option<Vec<&'s str>> {
        let mut missing = required
            .iter()
            .filter(|p| !obj.contains_key(p.as_str()))
            .map(|p| p.as_str());
        if self.bool_result {
            missing.next().map(|_| Vec::new())
        } else {
            let missing = missing.collect::<Vec<_>>();
            if missing.is_empty() {
                None
            } else {
                Some(missing)
            }
        }
    }

    fn instance_location(&self) -> InstanceLocation<'v> {
        let len = self.scope.vid;
        let mut tokens = Vec::with_capacity(len);
        for tok in &self.vloc[..len] {
            tokens.push(tok.clone());
        }
        InstanceLocation { tokens }
    }
}

#[derive(Default)]
struct Uneval<'v> {
    props: HashSet<&'v String>,
    items: HashSet<usize>,
}

impl<'v> Uneval<'v> {
    fn is_empty(&self) -> bool {
        self.props.is_empty() && self.items.is_empty()
    }

    fn from(v: &'v Value, sch: &Schema, caller_needs: bool) -> Self {
        let mut uneval = Self::default();
        match v {
            Value::Object(obj) => {
                if !sch.all_props_evaluated
                    && (caller_needs || sch.unevaluated_properties.is_some())
                {
                    uneval.props = obj.keys().collect();
                }
            }
            Value::Array(arr)
                if !sch.all_items_evaluated
                    && (caller_needs || sch.unevaluated_items.is_some())
                    && sch.num_items_evaluated < arr.len() =>
            {
                uneval.items = (sch.num_items_evaluated..arr.len()).collect();
            }
            _ => (),
        }
        uneval
    }

    fn merge(&mut self, other: &Uneval) {
        self.props.retain(|p| other.props.contains(p));
        self.items.retain(|i| other.items.contains(i));
    }
}

#[derive(Debug)]
struct Scope<'a> {
    sch: SchemaIndex,

    ref_kw: Option<&'static str>,

    vid: usize,
    parent: Option<&'a Self>,
}

impl Scope<'_> {
    const fn child<'x>(
        &'x self,
        sch: SchemaIndex,
        ref_kw: Option<&'static str>,
        vid: usize,
    ) -> Scope<'x> {
        Scope {
            sch,
            ref_kw,
            vid,
            parent: Some(self),
        }
    }

    fn check_cycle(&self) -> Option<&Scope<'_>> {
        let mut scope = self.parent;
        while let Some(scp) = scope {
            if scp.vid != self.vid {
                break;
            }
            if scp.sch == self.sch {
                return Some(scp);
            }
            scope = scp.parent;
        }
        None
    }
}

#[derive(Debug, Clone)]
pub enum InstanceToken<'v> {
    Prop(Cow<'v, str>),

    Item(usize),
}

impl From<String> for InstanceToken<'_> {
    fn from(prop: String) -> Self {
        InstanceToken::Prop(prop.into())
    }
}

impl<'v> From<&'v str> for InstanceToken<'v> {
    fn from(prop: &'v str) -> Self {
        InstanceToken::Prop(prop.into())
    }
}

impl From<usize> for InstanceToken<'_> {
    fn from(index: usize) -> Self {
        InstanceToken::Item(index)
    }
}

#[derive(Debug, Default)]
pub struct InstanceLocation<'v> {
    pub tokens: Vec<InstanceToken<'v>>,
}

impl InstanceLocation<'_> {
    fn new() -> Self {
        Self::default()
    }

    fn clone_static(self) -> InstanceLocation<'static> {
        let mut tokens = Vec::with_capacity(self.tokens.len());
        for tok in self.tokens {
            let tok = match tok {
                InstanceToken::Prop(p) => InstanceToken::Prop(p.into_owned().into()),
                InstanceToken::Item(i) => InstanceToken::Item(i),
            };
            tokens.push(tok);
        }
        InstanceLocation { tokens }
    }
}

impl Display for InstanceLocation<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for tok in &self.tokens {
            f.write_char('/')?;
            match tok {
                InstanceToken::Prop(s) => f.write_str(&escape(s))?,
                InstanceToken::Item(i) => write!(f, "{i}")?,
            }
        }
        Ok(())
    }
}

impl<'s> ValidationError<'s, '_> {
    pub(crate) fn clone_static(self) -> ValidationError<'s, 'static> {
        let mut causes = Vec::with_capacity(self.causes.len());
        for cause in self.causes {
            causes.push(cause.clone_static());
        }
        ValidationError {
            instance_location: self.instance_location.clone_static(),
            kind: self.kind.clone_static(),
            causes,
            ..self
        }
    }
}

impl<'s> ErrorKind<'s, '_> {
    fn clone_static(self) -> ErrorKind<'s, 'static> {
        use ErrorKind::*;
        match self {
            AdditionalProperties { got } => AdditionalProperties {
                got: got.into_iter().map(|e| e.into_owned().into()).collect(),
            },
            Format { got, want, err } => Format {
                got: Cow::Owned(got.into_owned()),
                want,
                err,
            },
            Pattern { got, want } => Pattern {
                got: got.into_owned().into(),
                want,
            },
            Minimum { got, want } => Minimum {
                got: Cow::Owned(got.into_owned()),
                want,
            },
            Maximum { got, want } => Maximum {
                got: Cow::Owned(got.into_owned()),
                want,
            },
            ExclusiveMinimum { got, want } => ExclusiveMinimum {
                got: Cow::Owned(got.into_owned()),
                want,
            },
            ExclusiveMaximum { got, want } => ExclusiveMaximum {
                got: Cow::Owned(got.into_owned()),
                want,
            },
            MultipleOf { got, want } => MultipleOf {
                got: Cow::Owned(got.into_owned()),
                want,
            },

            Group => Group,
            Schema { url } => Schema { url },
            ContentSchema => ContentSchema,
            PropertyName { prop } => PropertyName { prop },
            Reference { kw, url } => Reference { kw, url },
            RefCycle {
                url,
                kw_loc1,
                kw_loc2,
            } => RefCycle {
                url,
                kw_loc1,
                kw_loc2,
            },
            FalseSchema => FalseSchema,
            Type { got, want } => Type { got, want },
            Enum { want } => Enum { want },
            Const { want } => Const { want },
            MinProperties { got, want } => MinProperties { got, want },
            MaxProperties { got, want } => MaxProperties { got, want },
            Required { want } => Required { want },
            Dependency { prop, missing } => Dependency { prop, missing },
            DependentRequired { prop, missing } => DependentRequired { prop, missing },
            MinItems { got, want } => MinItems { got, want },
            MaxItems { got, want } => MaxItems { got, want },
            Contains => Contains,
            MinContains { got, want } => MinContains { got, want },
            MaxContains { got, want } => MaxContains { got, want },
            UniqueItems { got } => UniqueItems { got },
            AdditionalItems { got } => AdditionalItems { got },
            MinLength { got, want } => MinLength { got, want },
            MaxLength { got, want } => MaxLength { got, want },
            ContentEncoding { want, err } => ContentEncoding { want, err },
            ContentMediaType { got, want, err } => ContentMediaType { got, want, err },
            Not => Not,
            AllOf => AllOf,
            AnyOf => AnyOf,
            OneOf(opt) => OneOf(opt),
        }
    }
}
