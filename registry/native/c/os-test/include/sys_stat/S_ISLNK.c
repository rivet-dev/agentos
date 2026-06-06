#include <sys/stat.h>
#ifndef S_ISLNK
#error "S_ISLNK is not defined"
#endif
int main(void) { return 0; }
