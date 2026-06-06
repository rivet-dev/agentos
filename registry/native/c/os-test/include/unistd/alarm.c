#include <unistd.h>
#ifdef alarm
#undef alarm
#endif
unsigned (*foo)(unsigned) = alarm;
int main(void) { return 0; }
