/* Test whether a basic cfsetispeed invocation works. */

#include <termios.h>

#include "../basic.h"

int main(void)
{
	struct termios tio = {0};
	if ( cfsetispeed(&tio, B9600) < 0 )
		err(1, "cfsetispeed");
	return 0;
}
