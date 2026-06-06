#include <unistd.h>
#ifdef sleep
#undef sleep
#endif
unsigned (*foo)(unsigned) = sleep;
int main(void) { return 0; }
