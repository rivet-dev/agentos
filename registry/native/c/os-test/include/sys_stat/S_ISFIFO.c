#include <sys/stat.h>
#ifndef S_ISFIFO
#error "S_ISFIFO is not defined"
#endif
int main(void) { return 0; }
