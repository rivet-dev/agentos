/* Test whether a basic cfsetospeed invocation works. */

#include <termios.h>

#include "../basic.h"

int main(void)
{
	struct termios tio = {0};
	if ( cfsetospeed(&tio, B9600) < 0 )
		err(1, "cfsetospeed");
	return 0;
}
