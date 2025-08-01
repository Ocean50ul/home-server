// DEPRICATED
// rewrote it.

/***********************************************************************************************
 * Implementation of basic traits for Track.                                                   *
 *                                                                                             *
 * Display for Track;                                                                          *
 * Debug for Track;                                                                            *
*==============================================================================================*/
impl Display for Track {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let representation = format!(
            "Track [id: {}, name: {}, album_id: {}, date_added: {}, duration: {}, file_path: {}, file_size: {}, file_type: {}, uploaded: {}]",
            self.id, self.name, self.album_id, self.date_added.unwrap_or(NaiveDateTime::default()), self.duration, self.file_path.to_string_lossy(), self.file_size, self.file_type.as_str(), self.uploaded
        );
        write!(f, "{}", representation)
    }
}

impl Debug for Track {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {

        let make_space = |count: usize| " ".repeat(count);

        let fields = [
            ["id:".to_string(),        "file_type:".to_string(),     "date_added:".to_string()],
            ["title:".to_string(),     "file_size:".to_string(),  "duration:".to_string()],
            ["album_id:".to_string(), "file_path:".to_string(), "uploaded:".to_string()]
        ];

        let values = [
            [&self.id.to_string(), self.file_type.as_str(), &self.date_added.unwrap_or(NaiveDateTime::default()).to_string()],
            [&self.name, &self.file_size.to_string(), &self.duration.to_string()],
            [&self.album_id.to_string(), &self.file_path.to_string_lossy(), &self.uploaded.to_string()]
        ];

        let values_paddings = self.get_paddings(&values);
        let field_paddings = self.get_paddings(&fields);

        for col in 0..3 {
            for row in 0..3 {
                let field = fields[col][row].yellow().to_string();
                write!(f, "{}{}{}{}", 
                    field, 
                    make_space(field_paddings[col][row] + 1),
                    values[col][row],
                    make_space(values_paddings[col][row] + 1))?;
                }
            writeln!(f)?;
        }

        Ok(())

    }
}

/***********************************************************************************************
 * Definition of Filter trait that handles interface for EntityFilers structs.                 *
 *                                                                                             *
 * get_fields -- returns Vec of structs (field_name as &str, field_value as String)            *
 * default prepare_where_cluase -- returns raw string that represents sql where clause         *
*==============================================================================================*/
pub trait Filter {
    fn get_fields(&self) -> Vec<(&str, String)>;
    fn prepare_where_clauses(&self) -> (String, Vec<String>) {
        let fields = self.get_fields();

        if fields.is_empty() {
            return (String::new(), vec![])
        }

        let where_clauses: Vec<String> = fields 
            .iter()
            .map(|(field, _)| format!("{} = ?", field))
            .collect();

        let binds: Vec<String> = fields
            .into_iter()
            .map(|(_, value)| value)
            .collect();

        (format!(" WHERE {}", where_clauses.join(" AND ")), binds)
    }
}

/***********************************************************************************************
 * Filter structs reoresenting filter options.                                                 *
 *                                                                                             *
 * TrackFilters                                                                                *
 * AlbumFilters                                                                                *
 * ArtistFilters                                                                               *
*==============================================================================================*/
pub struct TrackFilters {
    pub id: Option<String>,
    pub name: Option<String>,
    pub album_id: Option<String>,
    pub duration: Option<u32>,

    pub file_path: Option<String>,
    pub file_size: Option<u32>,
    pub file_type: Option<String>,

    pub uploaded: Option<Uploaded>,
    pub date_added: Option<NaiveDateTime>
}

pub struct AlbumFilters {
    pub id: Option<String>,
    pub name: Option<String>,
    pub artist: Option<String>,
    pub year: Option<i64>
}

pub struct ArtistFilters {
    pub id: Option<String>,
    pub name: Option<String>
}

/***********************************************************************************************
 * Implementations of Default trait for filters structs.                                       *
*==============================================================================================*/
impl Default for TrackFilters {
    fn default() -> Self {
        Self {
            id: None,
            name: None,
            album_id: None,
            duration: None,
            file_path: None,
            file_size: None,
            file_type: None,
            uploaded: None,
            date_added: None
        }
    }
}

impl Default for AlbumFilters {
    fn default() -> Self {
        Self {
            id: None,
            name: None,
            artist: None,
            year: None
        }
    }
}


impl Default for ArtistFilters {
    fn default() -> Self {
        Self {
            id: None,
            name: None
        }
    }
}

/***********************************************************************************************
 * Implementation of Filter trait.                                                             *
 *                                                                                             *
 * Filter for AlbumFilters                                                                     *
 * Filter for TrackFilters                                                                     *
 * Filter for ArtistFilters                                                                    *
 *                                                                                             *
 * Filter::get_fields -- returns Vec of ("{field_name}", filed_value)                          *
*==============================================================================================*/
impl Filter for AlbumFilters {
    fn get_fields(&self) -> Vec<(&str, String)> {
        let mut fields: Vec<(&str, String)> = Vec::with_capacity(3);

        if let Some(id) = &self.id {
            fields.push(("id", id.clone()));
        }

        if let Some(name) = &self.name {
            fields.push(("name", name.clone()));
        }

        if let Some(artist) = &self.artist {
            fields.push(("artist", artist.clone()));
        }

        if let Some(year) = &self.year {
            fields.push(("year", year.to_string()));
        }

        fields
    }
}

impl Filter for TrackFilters {
    fn get_fields(&self) -> Vec<(&str, String)> {
        let mut fields: Vec<(&str, String)> = Vec::with_capacity(8);

        if let Some(id) = &self.id {
            fields.push(("id", id.clone()));
        }

        if let Some(name) = &self.name {
            fields.push(("name", name.clone()));
        }

        if let Some(album_id) = &self.album_id {
            fields.push(("album_id", album_id.clone()));
        }

        if let Some(duration) = &self.duration {
            fields.push(("duration", duration.to_string()));
        }

        if let Some(file_path) = &self.file_path {
            fields.push(("file_path", file_path.clone()));
        }

        if let Some(file_size) = &self.file_size {
            fields.push(("file_size", file_size.to_string()));
        }

        if let Some(file_type) = &self.file_type {
            fields.push(("file_type", file_type.clone()));
        }

        if let Some(uploaded) = &self.uploaded {
            fields.push(("uploaded", uploaded.to_string()));
        }

        if let Some(date_added) = &self.date_added {
            fields.push(("date_added", date_added.to_string()));
        }

        fields
    }
}

impl Filter for ArtistFilters {
    fn get_fields(&self) -> Vec<(&str, String)> {
        let mut fields: Vec<(&str, String)> = Vec::with_capacity(1);

        if let Some(id) = &self.id {
            fields.push(("id", id.to_string()));
        }

        if let Some(name) = &self.name {
            fields.push(("name", name.to_string()));
        }

        fields
    }
}

// For Track Display;
fn get_paddings<S>(&self, matrix: &[[S; 3]; 3]) -> Vec<Vec<usize>> 
where S: AsRef<str>
{
    let maxes: Vec<usize> = (0..3).map(|col_ind| {
        matrix.iter()
            .map(|row| row[col_ind].as_ref().len())
            .max()
            .unwrap_or(0)
    }).collect();

    matrix.iter()
        .map(|row| {
            row.iter().enumerate()
                .map(|(col_i, s)| {
                    maxes[col_i] - s.as_ref().len()
                })
                .collect::<Vec<usize>>()
        })
        .collect()
}