/*[XSI]*/
/* Test whether a basic dbm_close invocation works. */

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
	dbm_close(db);
	return 0;
}
