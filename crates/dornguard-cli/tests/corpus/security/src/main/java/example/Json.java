package example;

import com.fasterxml.jackson.annotation.JsonSubTypes;
import com.fasterxml.jackson.annotation.JsonTypeInfo;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.jsontype.BasicPolymorphicTypeValidator;
import com.fasterxml.jackson.databind.jsontype.impl.LaissezFaireSubTypeValidator;

class Json {
    ObjectMapper legacy(ObjectMapper mapper) {
        mapper.enableDefaultTyping(); // expect: security.jackson-default-typing
        mapper.enableDefaultTyping(ObjectMapper.DefaultTyping.NON_FINAL); // expect: security.jackson-default-typing
        return mapper;
    }

    ObjectMapper anyClass(ObjectMapper mapper) {
        return mapper.activateDefaultTyping(LaissezFaireSubTypeValidator.instance, ObjectMapper.DefaultTyping.EVERYTHING); // expect: security.jackson-default-typing
    }

    // A validator that allows only the application's packages is the safe form.
    ObjectMapper restricted(ObjectMapper mapper) {
        var validator = BasicPolymorphicTypeValidator.builder().allowIfSubType("example.").build();
        return mapper.activateDefaultTyping(validator, ObjectMapper.DefaultTyping.NON_FINAL);
    }
}

@JsonTypeInfo(use = JsonTypeInfo.Id.CLASS) // expect: security.jackson-default-typing
interface Payload {}

@JsonTypeInfo(include = JsonTypeInfo.As.PROPERTY, use = JsonTypeInfo.Id.MINIMAL_CLASS) // expect: security.jackson-default-typing
interface Event {}

// Logical type names with listed subtypes are safe.
@JsonTypeInfo(use = JsonTypeInfo.Id.NAME, property = "type")
@JsonSubTypes({@JsonSubTypes.Type(value = Circle.class, name = "circle")})
interface Shape {}

final class Circle implements Shape {}
