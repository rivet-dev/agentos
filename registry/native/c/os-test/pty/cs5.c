/* Test if CS5 has any effect on pseudoterminals. */

#include "suite.h"

#ifndef CBAUD
#define CBAUD 0
#endif

int main(void)
{
	int controller = posix_openpt(O_RDWR | O_NOCTTY);
	if ( controller < 0 )
		err(1, "posix_openpt");
	if ( grantpt(controller) < 0 )
		err(1, "grantpt");
	if ( unlockpt(controller) < 0 )
		err(1, "unlockpt");
	char* name = ptsname(controller);
	if ( !name )
		err(1, "unlockpt");
	int pty = open(name, O_RDWR | O_NOCTTY);
	if ( pty < 0 )
		err(1, "%s", name);
	struct termios tio;
	if ( tcgetattr(pty, &tio) < 0 )
		err(1, "tcgetattr");
	tio.c_iflag = 0;
	tio.c_oflag = 0;
	tio.c_cflag = CREAD | CS5 | (tio.c_cflag & CBAUD);
	tio.c_lflag = 0;
	tio.c_cc[VMIN] = 1;
	tio.c_cc[VTIME] = 0;
	if ( tcsetattr(pty, TCSANOW, &tio) < 0 )
		err(1, "tcsetattr");
	unsigned char request[] = { 0xF1, 0xF2, 0xF3, 0xF4 };
	if ( write(controller, request, 4) != 4 )
		err(1, "write");
	unsigned char data[4];
	if ( read(pty, data, 4) != 4 )
		err(1, "read");
	printf("%02x%02x%02x%02x\n", data[0], data[1], data[2], data[3]);
	fflush(stdout);
	if ( write(pty, request, 4) != 4 )
		err(1, "write 2");
	ssize_t amount;
	if ( (amount = read(controller, data, 4)) != 4 )
		err(1, "read 2: read %zi bytes", amount);
	printf("%02x%02x%02x%02x\n", data[0], data[1], data[2], data[3]);
	return 0;
}
