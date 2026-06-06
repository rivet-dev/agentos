#include <stdio.h>
#ifdef perror
#undef perror
#endif
void (*foo)(const char *) = perror;
int main(void) { return 0; }
