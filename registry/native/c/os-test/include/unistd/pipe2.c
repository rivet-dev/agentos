#include <unistd.h>
#ifdef pipe2
#undef pipe2
#endif
int (*foo)(int [2], int) = pipe2;
int main(void) { return 0; }
