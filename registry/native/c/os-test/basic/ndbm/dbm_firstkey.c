/*[XSI]*/
/* Test whether a basic dbm_firstkey invocation works. */

#include <sys/stat.h>

#include <fcntl.h>
#include <ndbm.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

static char* create_tmpdir(void)
{
	const char* tmpdir = getenv("TMPDIR");
	if ( !tmpdir )
		tmpdir = "/tmp";
	size_t template_len = strlen(tmpdir) + strlen("/os-test.XXXXXX");
	char* template = malloc(template_len + 1);
	if ( !template )
		err(1, "malloc");
	// mkdtemp is unfortunately less portable than link, so emulate it.
	while ( 1 )
	{
		strcpy(template, tmpdir);
		strcat(template, "/os-test.XXXXXX");
		int fd = mkstemp(template);
		if ( fd < 0 )
			err(1, "mkstemp");
		close(fd);
		if ( unlink(template) < 0 )
			err(1, "unlink");
		if ( mkdir(template, 0700) < 0 )
		{
			if ( errno == EEXIST )
				continue;
			err(1, "mkdir");
		}
		break;
	}
	return template;
}

static char* tmpdir;
static char* tmpdir_database;
static char* tmpdir_database_db;
static char* tmpdir_database_pag;
static char* tmpdir_database_dir;

static void cleanup(void)
{
	if ( tmpdir_database_db )
		unlink(tmpdir_database_db);
	if ( tmpdir_database_pag )
		unlink(tmpdir_database_pag);
	if ( tmpdir_database_dir )
		unlink(tmpdir_database_dir);
	if ( tmpdir )
		rmdir(tmpdir);
}

int main(void)
{
	if ( atexit(cleanup) )
		err(1, "atexit");
	tmpdir = create_tmpdir();
	tmpdir_database = malloc(strlen(tmpdir) + sizeof("/database"));
	tmpdir_database_db = malloc(strlen(tmpdir) + sizeof("/database.db"));
	tmpdir_database_pag = malloc(strlen(tmpdir) + sizeof("/database.pag"));
	tmpdir_database_dir = malloc(strlen(tmpdir) + sizeof("/database.dir"));
	if ( !tmpdir_database || !tmpdir_database_db || !tmpdir_database_pag ||
	     !tmpdir_database_dir )
		err(1, "malloc");
	strcpy(tmpdir_database, tmpdir);
	strcat(tmpdir_database, "/database");
	strcpy(tmpdir_database_db, tmpdir);
	strcat(tmpdir_database_db, "/database.db");
	strcpy(tmpdir_database_pag, tmpdir);
	strcat(tmpdir_database_pag, "/database.pag");
	strcpy(tmpdir_database_dir, tmpdir);
	strcat(tmpdir_database_dir, "/database.dir");
	DBM* db = dbm_open(tmpdir_database, O_RDWR | O_CREAT, 0600);
	if ( !db )
		err(1, "dbm_open");
	datum fookey = { .dptr = "foo", .dsize = 3 };
	datum foodata = { .dptr = "FOO", .dsize = 3 };
	datum barkey = { .dptr = "bar", .dsize = 3 };
	datum bardata = { .dptr = "BAR", .dsize = 3 };
	datum quxkey = { .dptr = "qux", .dsize = 3 };
	datum quxdata = { .dptr = "QUX", .dsize = 3 };
	int ret;
	datum lookup;
	// Test the first key of an empty database.
	lookup = dbm_firstkey(db);
	if ( lookup.dptr )
		errx(1, "first dbm_firstkey found absent key in empty database");
	if ( dbm_error(db) )
		errx(1, "first dbm_firstkey");
	// Try insert foo.
	if ( (ret = dbm_store(db, fookey, foodata, DBM_INSERT)) < 0 )
		err(1, "dbm_store foo");
	else if ( ret == 1 )
		errx(1, "dbm_store foo found absent entry");
	else if ( ret != 0 )
		errx(1, "dbm_store foo returned weird");
	// Test the first key after insertion of foo.
	lookup = dbm_firstkey(db);
	if ( !lookup.dptr )
	{
		if ( dbm_error(db) )
			err(1, "second dbm_firstkey");
		errx(1, "second dbm_firstkey did not find foo");
	}
	if ( lookup.dsize != 3 )
		errx(1, "second dbm_firstkey had wrong size");
	if ( memcmp(lookup.dptr, fookey.dptr, 3) != 0 )
		errx(1, "second dbm_firstkey had wrong key");
	// Try insert bar.
	if ( (ret = dbm_store(db, barkey, bardata, DBM_INSERT)) < 0 )
		err(1, "dbm_store bar");
	else if ( ret == 1 )
		errx(1, "dbm_store bar found absent entry");
	else if ( ret != 0 )
		errx(1, "dbm_store bar returned weird");
	// Try insert qux.
	if ( (ret = dbm_store(db, quxkey, quxdata, DBM_INSERT)) < 0 )
		err(1, "dbm_store qux");
	else if ( ret == 1 )
		errx(1, "dbm_store qux found absent entry");
	else if ( ret != 0 )
		errx(1, "dbm_store qux returned weird");
	// Test the first key after insertion of foo, bar, and qux.
	lookup = dbm_firstkey(db);
	if ( !lookup.dptr )
	{
		if ( dbm_error(db) )
			err(1, "third dbm_firstkey");
		errx(1, "third dbm_firstkey did not find a key");
	}
	if ( lookup.dsize != 3 )
		errx(1, "third dbm_firstkey had wrong size");
	if ( memcmp(lookup.dptr, fookey.dptr, 3) != 0 &&
	     memcmp(lookup.dptr, barkey.dptr, 3) != 0 &&
	     memcmp(lookup.dptr, quxkey.dptr, 3) != 0 )
		errx(1, "third dbm_firstkey had unknown key");
	dbm_close(db);
	return 0;
}
