package org.sidejump.cpid;

public class Thing extends OtherThing {
    private List<String> things;
    private Set<String> otherThings;
    protected final int DEFAULT_BATCH_SIZE = 1000L;
    private final String aLabel = "theLabelText";
    public final String bLabel = """
        multi
        line
        label
        text
        """;
}

