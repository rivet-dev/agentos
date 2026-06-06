#include <threads.h>
#ifdef call_once
#undef call_once
#endif
void (*foo)(once_flag *, void (*)(void)) = call_once;
int main(void) { return 0; }
