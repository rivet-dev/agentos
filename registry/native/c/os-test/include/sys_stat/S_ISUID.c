#include <sys/stat.h>
#ifndef S_ISUID
#error "S_ISUID is not defined"
#endif
int main(void) { return 0; }
