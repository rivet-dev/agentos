/*[SPN]*/
/* Test whether a basic posix_spawnp invocation works. */

#include <sys/stat.h>
#include <sys/wait.h>

#include <fcntl.h>
#include <spawn.h>
#include <signal.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

extern char** environ;

static char* once;
static char* temporary;

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

static void cleanup(void)
{
	if ( once )
		unlink(once);
	if ( temporary )
		rmdir(temporary);
}

int main(int argc, char* argv[])
{
	if ( argc == 2 )
	{
		if ( strcmp(argv[1], "success") != 0 )
			err(1, "child invoked incorrectly");
		if ( !getenv("OS_TEST_POSIX_SPAWNP") )
			errx(1, "$OS_TEST_POSIX_SPAWNP unset");
		// Test that the once file was created.
		struct stat st;
		if ( fstat(4, &st) < 0 )
			err(1, "fd 4 was not open");
		return 0;
	}
	// fd 4 is used to test if a file is created exactly once.
	close(4);
	// Create a temporary directory to contain the once file.
	if ( atexit(cleanup) )
		err(1, "atexit");
	temporary = create_tmpdir();
	// Test that the file actions are only run once by creating a 'once' file
	// in the temporary directory with O_EXCL. The PATH search may cause
	// posix_spawnp to call posix_spawn multiple times on a naive implementation
	// which is incorrect.
	once = malloc(strlen(temporary) + 1 + strlen("once") + 1);
	if ( !once )
		err(1, "malloc");
	strcpy(once, temporary);
	strcat(once, "/once");
	posix_spawn_file_actions_t actions;
	if ( (errno = posix_spawn_file_actions_init(&actions)) )
		err(1, "posix_spawn_file_actions_init");
	// "This transformation shall be as if the specified sequence of actions was
	//  performed exactly once, in the context of the spawned process (prior to
	//  execution of the new process image), in the order in which the actions
	//  were added to the object;"
	int flags = O_WRONLY | O_CREAT | O_EXCL;
	if ( (errno = posix_spawn_file_actions_addopen(&actions, 4, once, flags,
	                                               0600)) )
		err(1, "posix_spawn_file_actions_addopen");
	// Test that posix_spawnp searches PATH.
	char* new_path = malloc(strlen(temporary) + strlen(":spawn") + 1);
	if ( !new_path )
		err(1, "malloc");
	strcpy(new_path, temporary);
	strcat(new_path, ":spawn");
	if ( setenv("PATH", new_path, 1) < 0 )
		err(1, "setenv");
	// Test the environment is properly inherited.
	if ( setenv("OS_TEST_POSIX_SPAWNP", "set", 1) < 0 )
		err(1, "setenv");
	const char* program = "posix_spawnp";
	char* new_argv[] =
	{
		"posix_spawnp_child", // Does not exist, do not use.
		"success",
		NULL,
	};
	pid_t pid;
	if ( (errno = posix_spawnp(&pid, program, &actions, NULL, new_argv,
	                           environ)) )
		err(1, "posix_spawnp: %s", program);
	int status;
	if ( waitpid(pid, &status, 0) < 0 )
		err(1, "waitpid");
	if ( WIFEXITED(status) )
		return WEXITSTATUS(status);
	else if ( WIFSIGNALED(status) )
		errx(1, "%s", strsignal(WTERMSIG(status)));
	else
		errx(1, "unknown exit: %#x", status);
	return 0;
}
