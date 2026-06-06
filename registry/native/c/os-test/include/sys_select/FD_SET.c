#include <sys/select.h>
#ifndef FD_SET
void (*foo)(int, fd_set *) = FD_SET;
#endif
int main(void) { return 0; }
