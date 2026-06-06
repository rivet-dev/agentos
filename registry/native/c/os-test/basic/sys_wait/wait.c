/* Test whether a basic wait invocation works. */

#include <sys/wait.h>

#include <errno.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	// Make a child that exits 0.
	pid_t child1 = fork();
	if ( child1 < 0 )
		err(1, "first fork");
	if ( !child1 )
		_exit(0);
	// Make another child that exits 0.
	pid_t child2 = fork();
	if ( child2 < 0 )
		err(1, "second fork");
	if ( !child2 )
		_exit(0);
	// Wait for a child to exit.
	int status1;
	pid_t wait1 = wait(&status1);
	if ( wait1 < 0 )
		err(1, "first wait");
	if ( wait1 != child1 && wait1 != child2 )
		errx(1, "first wait gave strange child");
	if ( !WIFEXITED(status1) || WEXITSTATUS(status1) != 0 )
		errx(1, "%s child did not exit 0", wait1 == child1 ? "first" : "second");
	// Wait for the other child to exit.
	int status2;
	pid_t wait2 = wait(&status2);
	if ( wait2 < 0 )
		err(1, "second wait");
	if ( wait2 != child1 && wait2 != child2 )
		errx(1, "second wait gave strange child");
	if ( wait1 == wait2 )
		errx(1, "second wait gave the same child");
	if ( !WIFEXITED(status2) || WEXITSTATUS(status2) != 0 )
		errx(1, "%s child did not exit 0", wait2 == child1 ? "first" : "second");
	// Test wait with no children left.
	int status3;
	if ( wait(&status3) < 0 )
	{
		if ( errno != ECHILD )
			err(1, "third wait");
	}
	else
		errx(1, "third wait succeeded with no children");
	return 0;
}
