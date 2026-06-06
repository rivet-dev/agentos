/* Test whether a basic catopen invocation works. */

#include <sys/stat.h>
#include <sys/wait.h>

#include <nl_types.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

static char* temporary;
static char* msg_path;
static char* cat_path;

static void cleanup(void)
{
	if ( cat_path )
		unlink(cat_path);
	if ( msg_path )
		unlink(msg_path);
	if ( temporary )
		rmdir(temporary);
}

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

int main(void)
{
	// Create a message catalog in a temporary directory.
	if ( atexit(cleanup) )
		err(1, "atexit");
	temporary = create_tmpdir();
	const char* msg_suffix = "/nl_types.msg";
	const char* cat_suffix = "/nl_types.cat";
	msg_path = malloc(strlen(temporary) + strlen(msg_suffix) + 1);
	if ( !msg_path )
		err(1, "malloc");
	strcpy(msg_path, temporary);
	strcat(msg_path, msg_suffix);
	cat_path = malloc(strlen(temporary) + strlen(cat_suffix) + 1);
	if ( !cat_path )
		err(1, "malloc");
	strcpy(cat_path, temporary);
	strcat(cat_path, cat_suffix);
	// Create the source code for the message catalog.
	FILE* msg_fp = fopen(msg_path, "w");
	if ( !msg_fp )
		err(1, "$TMPDIR%s", msg_suffix);
	fprintf(msg_fp, "$set 1 first\n");
	fprintf(msg_fp, "1 One\n");
	fprintf(msg_fp, "$set 2 second\n");
	fprintf(msg_fp, "1 Uno\n");
	fprintf(msg_fp, "2 Dos\n");
	fprintf(msg_fp, "3 Tres\n");
	if ( ferror(msg_fp) || fflush(msg_fp) == EOF )
		err(1, "$TMPDIR%s", msg_suffix);
	fclose(msg_fp);
	// Compile the message catalog.
	pid_t child = fork();
	if ( child < 0 )
		err(1, "fork");
	if ( !child )
	{
		execlp("gencat", "gencat", cat_path, msg_path, (char*) NULL);
		err(127, "gencat");
	}
	int status;
	if ( waitpid(child, &status, 0) < 0 )
		err(1, "waitpid");
	if ( WIFEXITED(status) && WEXITSTATUS(status) )
		return WEXITSTATUS(status);
	else if ( WIFSIGNALED(status) )
		errx(1, "%s", strsignal(WTERMSIG(status)));
	else if ( !WIFEXITED(status) )
		errx(1, "unknown exit: %#x", status);
	// Open the catalog.
	nl_catd cat = catopen(cat_path, 0);
	if ( cat == (nl_catd) -1 )
		err(1, "catopen");
	return 0;
}
