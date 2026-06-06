#include <unistd.h>
#ifdef pipe
#undef pipe
#endif
int (*foo)(int [2]) = pipe;
int main(void) { return 0; }
