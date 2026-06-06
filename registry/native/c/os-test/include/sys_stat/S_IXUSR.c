#include <sys/stat.h>
#ifndef S_IXUSR
#error "S_IXUSR is not defined"
#endif
int main(void) { return 0; }
